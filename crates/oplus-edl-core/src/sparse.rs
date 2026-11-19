//! Android sparse v1.0 metadata parser. Payloads remain file-backed and bounded.
use crate::program_xml::Unsupported;
use anyhow::{Context, Result, bail};
use std::io::{Read, Seek, SeekFrom};
pub const MAGIC: u32 = 0xed26ff3a;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    Raw { file_offset: u64 },
    Fill([u8; 4]),
    DontCare,
}
#[derive(Debug, Clone)]
pub struct Chunk {
    pub offset: u64,
    pub bytes: u64,
    pub data: Data,
}
#[derive(Debug)]
pub struct Image {
    pub expanded_bytes: u64,
    pub transfer_bytes: u64,
    pub chunks: Vec<Chunk>,
}
fn u16le(b: &[u8]) -> u16 {
    u16::from_le_bytes(b[..2].try_into().unwrap())
}
fn u32le(b: &[u8]) -> u32 {
    u32::from_le_bytes(b[..4].try_into().unwrap())
}
pub fn parse(input: &mut (impl Read + Seek), sector: u64) -> Result<Image> {
    parse_controlled(input, sector, &|| Ok(()))
}

pub(crate) fn parse_controlled(
    input: &mut (impl Read + Seek),
    sector: u64,
    check: &impl Fn() -> Result<()>,
) -> Result<Image> {
    check()?;
    let file_size = input.seek(SeekFrom::End(0))?;
    input.seek(SeekFrom::Start(0))?;
    let mut header = [0; 28];
    input.read_exact(&mut header)?;
    let file_header = u16le(&header[8..]) as u64;
    let chunk_header = u16le(&header[10..]) as u64;
    let block = u32le(&header[12..]) as u64;
    let blocks = u32le(&header[16..]) as u64;
    let count = u32le(&header[20..]) as u64;
    if u32le(&header) != MAGIC {
        bail!("invalid sparse magic");
    }
    if u16le(&header[4..]) != 1 || u16le(&header[6..]) != 0 {
        return Err(Unsupported("unsupported sparse version".into()).into());
    }
    if file_header < 28
        || chunk_header < 12
        || file_header > file_size
        || sector == 0
        || block == 0
        || !block.is_multiple_of(4)
    {
        bail!("invalid sparse geometry");
    }
    if count > (file_size - file_header) / chunk_header {
        bail!("truncated sparse chunk headers");
    }
    let expanded_bytes = blocks.checked_mul(block).context("sparse size overflow")?;
    let mut offset = 0u64;
    let mut transfer_bytes = 0u64;
    let mut chunks = Vec::new();
    input.seek(SeekFrom::Start(file_header))?;
    for _ in 0..count {
        check()?;
        let pos = input.stream_position()?;
        let mut h = [0; 12];
        input.read_exact(&mut h)?;
        let bytes = (u32le(&h[4..]) as u64)
            .checked_mul(block)
            .context("chunk size overflow")?;
        let total = u32le(&h[8..]) as u64;
        let end = pos.checked_add(total).context("chunk end overflow")?;
        if total < chunk_header || end > file_size {
            bail!("invalid sparse chunk range");
        }
        let payload = total - chunk_header;
        input.seek(SeekFrom::Start(pos + chunk_header))?;
        let data = match u16le(&h) {
            0xcac1 if payload == bytes => Data::Raw {
                file_offset: pos + chunk_header,
            },
            0xcac2 if payload == 4 => {
                let mut value = [0; 4];
                input.read_exact(&mut value)?;
                Data::Fill(value)
            }
            0xcac3 if payload == 0 => Data::DontCare,
            0xcac1..=0xcac3 => bail!("invalid sparse payload length"),
            kind => {
                return Err(
                    Unsupported(format!("unsupported sparse chunk type: {kind:#x}")).into(),
                );
            }
        };
        // Validate every payload first, including supported zero-length chunks.
        // Sector alignment applies to expanded chunks, not sparse block size.
        if !bytes.is_multiple_of(sector) || !offset.is_multiple_of(sector) {
            bail!("sparse chunk is not sector aligned");
        }
        if bytes == 0 {
            input.seek(SeekFrom::Start(end))?;
            continue;
        }
        if data != Data::DontCare {
            transfer_bytes = transfer_bytes
                .checked_add(bytes)
                .context("transfer size overflow")?;
        }
        chunks.push(Chunk {
            offset,
            bytes,
            data,
        });
        offset = offset
            .checked_add(bytes)
            .context("sparse offset overflow")?;
        if offset > expanded_bytes {
            bail!("sparse chunks exceed declared blocks");
        }
        input.seek(SeekFrom::Start(end))?;
    }
    check()?;
    if offset != expanded_bytes || input.stream_position()? != file_size {
        bail!("sparse block count or file length mismatch");
    }
    Ok(Image {
        expanded_bytes,
        transfer_bytes,
        chunks,
    })
}

#[cfg(test)]
#[test]
fn preparation_stop_between_sparse_chunks_preserves_the_typed_cause() {
    use std::{cell::Cell, io::Cursor};

    let mut bytes = Vec::new();
    bytes.extend(MAGIC.to_le_bytes());
    for field in [1u16, 0, 28, 12] {
        bytes.extend(field.to_le_bytes());
    }
    for field in [512u32, 2, 2, 0] {
        bytes.extend(field.to_le_bytes());
    }
    for _ in 0..2 {
        bytes.extend(0xcac2u16.to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(16u32.to_le_bytes());
        bytes.extend([1, 2, 3, 4]);
    }
    assert_eq!(
        parse(&mut Cursor::new(&bytes), 512).unwrap().chunks.len(),
        2
    );

    let calls = Cell::new(0);
    let mut input = Cursor::new(&bytes);
    let error = parse_controlled(&mut input, 512, &|| {
        calls.set(calls.get() + 1);
        if calls.get() == 3 {
            return Err(crate::session::Stopped.into());
        }
        Ok(())
    })
    .unwrap_err();
    assert!(error.is::<crate::session::Stopped>());
    assert_eq!(input.position(), 28 + 16);
}
