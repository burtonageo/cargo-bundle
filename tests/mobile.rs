//! End-to-end checks for Apple signing inputs, IPAs and Android APKs, driven
//! through the CLI with prebuilt binaries so no target toolchain is needed.

use apple_codesign::{MachOBinary, MachoTarget, Platform, macho_builder::MachOBuilder};
use plist::{Dictionary, Value};
use std::{fs, path::Path, process::Command};

/// Writes `Cargo.toml` and returns a runner for `cargo bundle` in `dir`.
fn project(dir: &Path, metadata: &str) -> impl Fn(&[&str], &Path) -> std::process::Output {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"postcard\"\nversion = \"1.2.3\"\nedition = \"2024\"\n\
             [package.metadata.bundle]\nname = \"Post Card\"\nidentifier = \"com.example.postcard\"\n{metadata}"
        ),
    )
    .unwrap();
    let dir = dir.to_owned();
    move |args, binary| {
        Command::new(env!("CARGO_BIN_EXE_cargo-bundle"))
            .arg("bundle")
            .args(args)
            .arg("--binary-path")
            .arg(binary)
            .current_dir(&dir)
            .output()
            .unwrap()
    }
}

fn succeeded(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A self-signed code-signing identity exported as an unprotected PKCS#12 file.
fn write_identity(path: &Path) {
    let (certificate, key) = apple_codesign::create_self_signed_code_signing_certificate(
        x509_certificate::KeyAlgorithm::Ecdsa(x509_certificate::EcdsaCurve::Secp256r1),
        apple_codesign::CertificateProfile::AppleDevelopment,
        "TESTTEAM01",
        "cargo-bundle test",
        "US",
        chrono::Duration::hours(1),
    )
    .unwrap();
    let pfx = p12::PFX::new(
        certificate.constructed_data(),
        &key.to_pkcs8_one_asymmetric_key_der(),
        None,
        "",
        "test",
    )
    .unwrap();
    fs::write(path, pfx.to_der()).unwrap();
}

fn macho(platform: Platform) -> Vec<u8> {
    MachOBuilder::new_aarch64(2)
        .macho_target(MachoTarget {
            platform,
            minimum_os_version: "17.0.0".parse().unwrap(),
            sdk_version: "17.0.0".parse().unwrap(),
        })
        .write_macho()
        .unwrap()
}

fn plist_file(path: &Path) -> Dictionary {
    Value::from_file(path).unwrap().into_dictionary().unwrap()
}

/// A signature must seal the profile, so embedding it afterwards would break
/// verification; the claims must also survive XML and binary input plists.
#[test]
fn signed_bundles_seal_the_profile_and_entitlements() {
    let claims = Dictionary::from_iter([
        ("get-task-allow", Value::Boolean(true)),
        (
            "com.apple.security.application-groups",
            Value::Array(vec![Value::String("group.example".into())]),
        ),
    ]);
    // (format, target, platform, code root inside the bundle, executable, profile)
    let cases = [
        (
            "osx",
            "aarch64-apple-darwin",
            Platform::MacOs,
            "Contents",
            "Contents/MacOS/postcard",
            "embedded.provisionprofile",
        ),
        (
            "ios",
            "aarch64-apple-ios",
            Platform::IOs,
            "",
            "postcard",
            "embedded.mobileprovision",
        ),
    ];
    for (format, target, platform, root, executable, profile_name) in cases {
        for binary_plist in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let dir = temp.path();
            write_identity(&dir.join("identity.p12"));
            fs::write(dir.join("device.profile"), b"opaque \0 profile \xff bytes").unwrap();
            let entitlements = Value::Dictionary(claims.clone());
            if binary_plist {
                entitlements
                    .to_file_binary(dir.join("claims.plist"))
                    .unwrap();
            } else {
                entitlements.to_file_xml(dir.join("claims.plist")).unwrap();
            }
            let run = project(
                dir,
                "apple_signing_p12 = \"identity.p12\"\n\
                 apple_provisioning_profile = \"device.profile\"\n\
                 apple_signing_entitlements = \"claims.plist\"\n",
            );
            fs::write(dir.join("prebuilt"), macho(platform)).unwrap();
            succeeded(&run(
                &["--format", format, "--target", target],
                &dir.join("prebuilt"),
            ));

            let app = dir.join(format!(
                "target/{target}/debug/bundle/{format}/Post Card.app"
            ));
            let code_root = app.join(root);
            assert_eq!(
                fs::read(code_root.join(profile_name)).unwrap(),
                b"opaque \0 profile \xff bytes"
            );
            let resources = plist_file(&code_root.join("_CodeSignature/CodeResources"));
            assert!(
                resources["files2"]
                    .as_dictionary()
                    .unwrap()
                    .contains_key(profile_name),
                "{format}: the profile is not sealed by the signature"
            );
            let bytes = fs::read(app.join(executable)).unwrap();
            let macho = MachOBinary::parse(&bytes).unwrap();
            let signature = macho.code_signature().unwrap().unwrap();
            let signed = signature.entitlements().unwrap().unwrap();
            assert_eq!(
                Value::from_reader_xml(signed.as_str().as_bytes()).unwrap(),
                Value::Dictionary(claims.clone())
            );
            assert!(apple_codesign::verify_macho_data(&bytes).is_empty());
            if cfg!(target_os = "macos") && format == "osx" {
                let verified = Command::new("/usr/bin/codesign")
                    .args(["--verify", "--deep", "--strict"])
                    .arg(&app)
                    .output()
                    .unwrap();
                succeeded(&verified);
            }
        }
    }
}
