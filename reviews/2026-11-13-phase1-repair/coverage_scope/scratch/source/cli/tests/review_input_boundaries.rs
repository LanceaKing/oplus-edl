#[path = "../src/input.rs"]
mod input;

use input::{ProgramInput, zero_input};
use std::{fs, io::Read};

#[test]
fn selected_ranges_and_padding_match_small_exhaustive_matrix() {
    let dir = std::env::temp_dir().join(format!("qdlrs-review-input-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("image.bin");
    for source_len in 0..=21u64 {
        let data: Vec<_> = (0..source_len).map(|x| (x + 1) as u8).collect();
        fs::write(&path, &data).unwrap();
        for sector in 1..=8usize {
            for offset in 0..=source_len + 1 {
                for requested in std::iter::once(None).chain((0..=24usize).map(Some)) {
                    let valid_offset = offset <= source_len;
                    let available = source_len.saturating_sub(offset);
                    let sectors = requested.unwrap_or(available.div_ceil(sector as u64) as usize);
                    let target_len = sectors * sector;
                    let valid = valid_offset && (target_len as u64).saturating_sub(available) < sector as u64;
                    let result = ProgramInput::from_file(fs::File::open(&path).unwrap(), sector, offset, requested);
                    assert_eq!(result.is_ok(), valid, "len={source_len}, sector={sector}, offset={offset}, requested={requested:?}");
                    if let Ok(mut input) = result {
                        let source_end = (offset as usize + target_len).min(data.len());
                        let mut expected = data[offset as usize..source_end].to_vec();
                        expected.resize(target_len, 0);
                        let mut actual = Vec::new();
                        let mut one = [0; 1];
                        while input.read(&mut one).unwrap() != 0 { actual.push(one[0]); }
                        assert_eq!(input.num_sectors(), sectors);
                        assert_eq!(actual, expected);
                        assert_eq!(input.read(&mut one).unwrap(), 0);
                    }
                }
            }
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn truncation_inside_first_source_chunk_remains_an_error() {
    let dir = std::env::temp_dir().join(format!("qdlrs-review-truncated-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("image.bin");
    fs::write(&path, b"abc").unwrap();
    let mut input = ProgramInput::from_file(fs::File::open(&path).unwrap(), 4, 0, None).unwrap();
    fs::OpenOptions::new().write(true).open(&path).unwrap().set_len(2).unwrap();
    let mut data = Vec::new();
    assert_eq!(input.read_to_end(&mut data).unwrap_err().kind(), std::io::ErrorKind::UnexpectedEof);
    assert_eq!(data, b"ab");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn zero_stream_empty_reads_do_not_change_remaining_length() {
    let mut input = zero_input(3, 7).unwrap();
    assert_eq!(input.read(&mut []).unwrap(), 0);
    let mut data = Vec::new();
    input.read_to_end(&mut data).unwrap();
    assert_eq!(data, vec![0; 21]);
    assert_eq!(input.limit(), 0);
}
