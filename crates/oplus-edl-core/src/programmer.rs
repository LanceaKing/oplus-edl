// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
// Adapted from the pinned qdl programmer archive decoder for an already-open source.

use anyhow::{Context, Result, ensure};
use std::{io::Read, str};

const HEADER_LEN: usize = 110;
const MAGIC: &[u8] = b"070701";

pub(crate) fn load(mut source: impl Read) -> Result<Vec<Option<Vec<u8>>>> {
    let mut blob = Vec::new();
    source.read_to_end(&mut blob)?;
    if blob.len() < HEADER_LEN || !blob.starts_with(MAGIC) {
        return Ok(vec![Some(blob)]);
    }

    let mut images = Vec::new();
    let mut remaining = blob.as_slice();
    loop {
        let (header, after_header) = remaining
            .split_at_checked(HEADER_LEN)
            .context("programmer archive is truncated")?;
        ensure!(
            header.starts_with(MAGIC),
            "expected cpio header in programmer archive"
        );
        let length = hex_size(&header[54..62])?;
        let name_length = hex_size(&header[94..102])?;
        ensure!(
            name_length != 0,
            "missing filename in programmer archive entry"
        );
        let name = after_header
            .get(..name_length)
            .context("programmer archive is truncated")?;
        let name = str::from_utf8(name.split(|byte| *byte == 0).next().unwrap_or_default())
            .context("invalid utf8 in programmer archive filename")?;
        if name == "TRAILER!!!" {
            return Ok(images);
        }
        let id = name
            .split(':')
            .next()
            .unwrap_or_default()
            .parse::<u32>()
            .context("invalid decimal image id in programmer archive")? as usize;
        ensure!(id > 0, "invalid zero image id in programmer archive");
        // newc aligns the data relative to the header start, then pads the data to four bytes.
        let payload_start = (HEADER_LEN + name_length).next_multiple_of(4);
        let payload = remaining
            .get(payload_start..)
            .context("programmer archive is truncated")?;
        let image = payload
            .get(..length)
            .context("programmer archive is truncated")?;
        remaining = payload
            .get(length.next_multiple_of(4)..)
            .context("programmer archive is truncated")?;
        if id >= images.len() {
            let count = id
                .checked_add(1)
                .context("image id exceeds host index range")?;
            images.try_reserve(count - images.len())?;
            images.resize_with(count, || None);
        }
        images[id] = Some(image.to_vec());
    }
}

fn hex_size(field: &[u8]) -> Result<usize> {
    Ok(u32::from_str_radix(str::from_utf8(field)?, 16).context("invalid newc size")? as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(archive: &mut Vec<u8>, name: &str, payload: &[u8]) {
        let mut header = [b'0'; HEADER_LEN];
        header[..6].copy_from_slice(MAGIC);
        header[54..62].copy_from_slice(format!("{:08x}", payload.len()).as_bytes());
        header[94..102].copy_from_slice(format!("{:08x}", name.len() + 1).as_bytes());
        archive.extend_from_slice(&header);
        archive.extend_from_slice(name.as_bytes());
        archive.resize((archive.len() + 1).next_multiple_of(4), 0);
        archive.extend_from_slice(payload);
        archive.resize(archive.len().next_multiple_of(4), 0);
    }

    #[test]
    fn reads_the_single_image_and_newc_slots_the_same_way_as_the_pin() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("programmer.bin");
        let mut archive = Vec::new();
        member(&mut archive, "13:loader.elf", b"abc");
        member(&mut archive, "15", b"defg");
        member(&mut archive, "13", b"last image wins");
        member(&mut archive, "TRAILER!!!", b"");
        for fixture in [b"single image".as_slice(), &archive] {
            std::fs::write(&path, fixture).unwrap();
            let expected = qdl::load_programmer_images(&path).unwrap();
            assert_eq!(load(fixture).unwrap(), expected);
        }
        assert!(load(&archive[..HEADER_LEN + 2]).is_err());
    }
}
