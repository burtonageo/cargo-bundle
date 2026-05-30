#[cfg(any(target_os = "windows", test))]
use std::io::{self, Cursor, Write};

#[cfg(target_os = "windows")]
use super::group_icon::GroupIconEntry;

#[cfg(any(target_os = "windows", test))]
const BITMAP_INFO_HEADER_BYTE_SIZE: u32 = 40;
#[cfg(any(target_os = "windows", test))]
const DIB_SINGLE_COLOR_PLANE: u16 = 1;
#[cfg(any(target_os = "windows", test))]
const BITS_PER_BGRA_PIXEL: u16 = 32;
#[cfg(any(target_os = "windows", test))]
const BYTES_PER_BGRA_PIXEL: u32 = 4;
#[cfg(any(target_os = "windows", test))]
const BI_RGB_NO_COMPRESSION: u32 = 0;

/// Bit width of one DWORD, used to compute the AND mask row stride.
#[cfg(any(target_os = "windows", test))]
const AND_MASK_ROW_ALIGNMENT_BITS: u32 = 32;
#[cfg(any(target_os = "windows", test))]
const BYTES_PER_DWORD: u32 = 4;

#[cfg(any(target_os = "windows", test))]
const UNUSED_PELS_PER_METER: i32 = 0;
#[cfg(any(target_os = "windows", test))]
const UNUSED_COLOR_TABLE_ENTRIES: u32 = 0;

/// Offsets into the BITMAPINFOHEADER for the bits-per-pixel field, used in tests.
#[cfg(any(target_os = "windows", test))]
const BITMAPINFOHEADER_BITS_PER_PIXEL_OFFSET: usize = 14;

/// A single icon entry stored as a BITMAPINFOHEADER + bottom-up BGRA pixels +
/// AND mask, ready to be embedded as an RT_ICON resource in a PE file.
#[cfg(any(target_os = "windows", test))]
pub struct Icon {
    width: u32,
    height: u32,
    planes: u16,
    bits_per_pixel: u16,
    compression: u32,
    horizontal_pixels_per_meter: i32,
    vertical_pixels_per_meter: i32,
    color_table_size: u32,
    important_color_count: u32,
    image_data: Vec<u8>,
}

#[cfg(any(target_os = "windows", test))]
impl Icon {
    pub fn new(width: u32, height: u32, image_data: Vec<u8>) -> Self {
        Icon {
            width,
            height,
            planes: DIB_SINGLE_COLOR_PLANE,
            bits_per_pixel: BITS_PER_BGRA_PIXEL,
            compression: BI_RGB_NO_COMPRESSION,
            horizontal_pixels_per_meter: UNUSED_PELS_PER_METER,
            vertical_pixels_per_meter: UNUSED_PELS_PER_METER,
            color_table_size: UNUSED_COLOR_TABLE_ENTRIES,
            important_color_count: UNUSED_COLOR_TABLE_ENTRIES,
            image_data,
        }
    }

    /// Build an `Icon` from raw RGBA pixel data (top-to-bottom row order).
    /// The data is converted to bottom-up BGRA order as required by the DIB
    /// format used in PE icon resources.
    pub fn new_from_rgba(width: u32, height: u32, rgba_pixels: Vec<u8>) -> Self {
        let mut bgra_data = Vec::with_capacity((width * height * BYTES_PER_BGRA_PIXEL) as usize);

        for row in (0..height).rev() {
            for column in 0..width {
                let pixel_byte_offset = ((row * width + column) * BYTES_PER_BGRA_PIXEL) as usize;
                bgra_data.push(rgba_pixels[pixel_byte_offset + 2]); // B
                bgra_data.push(rgba_pixels[pixel_byte_offset + 1]); // G
                bgra_data.push(rgba_pixels[pixel_byte_offset]); // R
                bgra_data.push(rgba_pixels[pixel_byte_offset + 3]); // A
            }
        }

        Icon::new(width, height, bgra_data)
    }

    /// Encode this icon as a DIB (BITMAPINFOHEADER + pixel data + AND mask).
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut buffer = Cursor::new(Vec::new());

        let and_mask_row_stride =
            self.width.div_ceil(AND_MASK_ROW_ALIGNMENT_BITS) * BYTES_PER_DWORD;
        let and_mask_byte_size = and_mask_row_stride * self.height;
        let pixel_data_byte_size = self.width * self.height * BYTES_PER_BGRA_PIXEL;

        // BITMAPINFOHEADER
        buffer.write_all(&BITMAP_INFO_HEADER_BYTE_SIZE.to_le_bytes())?;
        buffer.write_all(&self.width.to_le_bytes())?;
        buffer.write_all(&(self.height * 2).to_le_bytes())?; // doubled: XOR plane + AND plane
        buffer.write_all(&self.planes.to_le_bytes())?;
        buffer.write_all(&self.bits_per_pixel.to_le_bytes())?;
        buffer.write_all(&self.compression.to_le_bytes())?;
        buffer.write_all(&(pixel_data_byte_size + and_mask_byte_size).to_le_bytes())?;
        buffer.write_all(&self.horizontal_pixels_per_meter.to_le_bytes())?;
        buffer.write_all(&self.vertical_pixels_per_meter.to_le_bytes())?;
        buffer.write_all(&self.color_table_size.to_le_bytes())?;
        buffer.write_all(&self.important_color_count.to_le_bytes())?;

        // XOR (colour) plane
        buffer.write_all(&self.image_data)?;

        // AND mask — all zeros because transparency is carried in the alpha channel
        buffer.write_all(&vec![0u8; and_mask_byte_size as usize])?;

        Ok(buffer.into_inner())
    }

    /// Return the `GroupIconEntry` that describes this icon in an `RT_GROUP_ICON` resource.
    #[cfg(target_os = "windows")]
    pub fn group_icon_entry(&self, id: u16) -> io::Result<GroupIconEntry> {
        Ok(GroupIconEntry {
            width: self.width as u8,
            height: self.height as u8,
            color_count: 0,
            reserved: 0,
            planes: self.planes,
            bit_count: self.bits_per_pixel,
            bytes_in_resource: self.encode()?.len() as u32,
            id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_rgba(size: u32) -> Vec<u8> {
        vec![0u8; (size * size * BYTES_PER_BGRA_PIXEL) as usize]
    }

    #[test]
    fn icon_encode_length_is_deterministic() {
        let size = 16u32;
        let icon = Icon::new_from_rgba(size, size, blank_rgba(size));
        let encoded = icon.encode().unwrap();

        let and_mask_row_stride = size.div_ceil(AND_MASK_ROW_ALIGNMENT_BITS) * BYTES_PER_DWORD;
        let expected = BITMAP_INFO_HEADER_BYTE_SIZE
            + (size * size * BYTES_PER_BGRA_PIXEL)
            + and_mask_row_stride * size;
        assert_eq!(encoded.len() as u32, expected);
    }

    #[test]
    fn icon_encode_header_signature() {
        let icon = Icon::new_from_rgba(32, 32, blank_rgba(32));
        let encoded = icon.encode().unwrap();

        let header_size = u32::from_le_bytes(encoded[0..4].try_into().unwrap());
        assert_eq!(header_size, BITMAP_INFO_HEADER_BYTE_SIZE);

        let bits_per_pixel = u16::from_le_bytes(
            encoded[BITMAPINFOHEADER_BITS_PER_PIXEL_OFFSET
                ..BITMAPINFOHEADER_BITS_PER_PIXEL_OFFSET + 2]
                .try_into()
                .unwrap(),
        );
        assert_eq!(bits_per_pixel, BITS_PER_BGRA_PIXEL);
    }

    #[test]
    fn rgba_to_bgra_conversion() {
        let opaque_red_rgba = vec![0xFF, 0x00, 0x00, 0xFF];
        let icon = Icon::new_from_rgba(1, 1, opaque_red_rgba);
        let encoded = icon.encode().unwrap();

        let first_pixel_offset = BITMAP_INFO_HEADER_BYTE_SIZE as usize;
        assert_eq!(encoded[first_pixel_offset], 0x00); // B
        assert_eq!(encoded[first_pixel_offset + 1], 0x00); // G
        assert_eq!(encoded[first_pixel_offset + 2], 0xFF); // R
        assert_eq!(encoded[first_pixel_offset + 3], 0xFF); // A
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn group_icon_entry_reflects_icon_metadata() {
        let icon = Icon::new_from_rgba(48, 48, blank_rgba(48));
        let entry = icon.group_icon_entry(5).unwrap();
        assert_eq!(entry.width, 48);
        assert_eq!(entry.height, 48);
        assert_eq!(entry.id, 5);
        assert_eq!(entry.bit_count, BITS_PER_BGRA_PIXEL);
    }
}
