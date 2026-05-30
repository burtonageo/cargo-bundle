#[cfg(target_os = "windows")]
use std::io::{self, Cursor, Write};

/// Reserved field value required by the GRPICONDIR specification.
#[cfg(target_os = "windows")]
const ICON_DIRECTORY_RESERVED: u16 = 0;

/// Type identifier distinguishing icon resources from cursor resources.
#[cfg(target_os = "windows")]
const ICON_DIRECTORY_TYPE_ICON: u16 = 1;

/// An RT_GROUP_ICON resource that groups together individual RT_ICON entries.
#[cfg(target_os = "windows")]
pub struct GroupIcon {
    id_reserved: u16,
    id_type: u16,
    entries: Vec<GroupIconEntry>,
}

#[cfg(target_os = "windows")]
pub struct GroupIconEntry {
    pub width: u8,
    pub height: u8,
    pub color_count: u8,
    pub reserved: u8,
    pub planes: u16,
    pub bit_count: u16,
    pub bytes_in_resource: u32,
    pub id: u16,
}

#[cfg(target_os = "windows")]
impl Default for GroupIcon {
    fn default() -> Self {
        GroupIcon {
            id_reserved: ICON_DIRECTORY_RESERVED,
            id_type: ICON_DIRECTORY_TYPE_ICON,
            entries: Vec::new(),
        }
    }
}

#[cfg(target_os = "windows")]
impl GroupIcon {
    pub fn push_icon(&mut self, entry: GroupIconEntry) {
        self.entries.push(entry);
    }

    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut buffer = Cursor::new(Vec::new());

        buffer.write_all(&self.id_reserved.to_le_bytes())?;
        buffer.write_all(&self.id_type.to_le_bytes())?;
        buffer.write_all(&(self.entries.len() as u16).to_le_bytes())?;

        for entry in &self.entries {
            buffer.write_all(&entry.width.to_le_bytes())?;
            buffer.write_all(&entry.height.to_le_bytes())?;
            buffer.write_all(&entry.color_count.to_le_bytes())?;
            buffer.write_all(&entry.reserved.to_le_bytes())?;
            buffer.write_all(&entry.planes.to_le_bytes())?;
            buffer.write_all(&entry.bit_count.to_le_bytes())?;
            buffer.write_all(&entry.bytes_in_resource.to_le_bytes())?;
            buffer.write_all(&entry.id.to_le_bytes())?;
        }

        Ok(buffer.into_inner())
    }
}
