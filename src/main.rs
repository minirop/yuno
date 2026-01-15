use byteorder::LittleEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use clap::Parser;
use encoding_rs::SHIFT_JIS;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Seek;
use std::io::Write;
use std::{fs::File, io::SeekFrom};

#[derive(clap::Args, Debug)]
#[group(required = true, multiple = false)]
struct Action {
    #[arg(short, long)]
    extract: Option<String>,

    #[arg(short, long)]
    patch: Option<String>,
}

#[derive(Debug, Parser)]
struct Args {
    filename: String,

    #[command(flatten)]
    action: Action,
}

const RANGES: [(u32, u32); 11] = [
    (0x020d0938, 0x020d4770),
    (0x020d725c, 0x020d8de4),
    (0x020db1d4, 0x020dc954),
    (0x020dee2c, 0x020e05d4),
    (0x020e3050, 0x020e4a88),
    (0x020e81c8, 0x020ea460),
    (0x020ebdd0, 0x020ecd78),
    (0x020ee6ec, 0x020ef684),
    (0x020f0e40, 0x020f1cd8),
    (0x020f3348, 0x020f4150),
    (0x020f5988, 0x020f67e8),
];
const DIFF: u32 = 0x1FFC000;
const TEXT_SPACE: (u32, u32) = (0x02106ad8, 0x0213e3a8);

fn main() -> std::io::Result<()> {
    let args = Args::parse();

    if let Some(filename) = args.action.extract {
        let mut rom = File::open(&args.filename)?;
        let mut file = File::create(format!("{filename}.sae"))?;
        let mut line_count = 0;
        for (start, finish) in RANGES {
            rom.seek(SeekFrom::Start((start - DIFF) as u64))?;

            for offset in (start..finish).step_by(8) {
                let addr = rom.read_u32::<LittleEndian>()?;
                let _ = rom.read_u16::<LittleEndian>()?;
                let _ = rom.read_u16::<LittleEndian>()?;
                let string = read_string(&mut rom, addr - DIFF)?;

                writeln!(file, "{offset:X},{}", string.replace("\n", "\\n"))?;
                line_count += 1;
            }
        }

        println!("extracted {line_count} lines");
    } else if let Some(filename) = args.action.patch {
        let mut rom = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&args.filename)?;
        let mut file = File::open(format!("{filename}.sae"))?;

        let required_size = calculating_required_size(&mut file)?;
        let available_size = TEXT_SPACE.1 - TEXT_SPACE.0;

        if required_size <= available_size {
            patch_strings(&mut rom, &mut file)?;
        } else {
            println!("Too much text, not enough free space.");
        }
    }

    Ok(())
}

fn read_string(file: &mut File, address: u32) -> std::io::Result<String> {
    let pos = file.seek(SeekFrom::Current(0))?;
    file.seek(SeekFrom::Start(address as u64))?;

    let mut bytes = vec![];

    while let Ok(b) = file.read_u8() {
        if b == 0 {
            break;
        }

        bytes.push(b);
    }

    let (res, _, errors) = SHIFT_JIS.decode(&bytes);
    assert!(!errors);
    let string = res.into_owned();

    file.seek(SeekFrom::Start(pos))?;
    Ok(string)
}

fn patch_strings(rom: &mut File, file: &mut File) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    let mut lines = HashMap::new();
    for buf in BufReader::new(file).lines() {
        let line = buf?;
        let mut elems = line.splitn(2, ",");
        let addr = u32::from_str_radix(elems.next().unwrap(), 16).unwrap();
        let string = elems.next().unwrap();
        let string = string.replace("\\n", "\n");
        lines.insert(addr, string);
    }

    let mut current_text_space_ptr = TEXT_SPACE.0 - DIFF;
    for (start, finish) in RANGES {
        for offset in (start..finish).step_by(8) {
            if let Some(string) = lines.get(&offset) {
                let this_text_pos = current_text_space_ptr;
                write_string(rom, &mut current_text_space_ptr, &string)?;
                rom.seek(SeekFrom::Start((offset - DIFF) as u64))?;
                rom.write_u32::<LittleEndian>(this_text_pos + DIFF)?;
            } else {
                panic!("String at offset {offset:X} is missing!");
            }
        }
    }

    println!(
        "current_text_space_ptr ended up at {:X}",
        current_text_space_ptr + DIFF
    );

    Ok(())
}

fn write_string(rom: &mut File, offset: &mut u32, string: &str) -> std::io::Result<()> {
    rom.seek(SeekFrom::Start(*offset as u64))?;
    let (sjis, _, errors) = SHIFT_JIS.encode(&string);
    assert!(!errors);
    rom.write(&sjis)?;
    rom.write_u8(0)?;
    *offset += sjis.len() as u32 + 1;

    while (*offset % 4) != 0 {
        rom.write_u8(0)?;
        *offset += 1;
    }

    Ok(())
}

fn calculating_required_size(file: &mut File) -> std::io::Result<u32> {
    let mut size = 0;
    for buf in BufReader::new(file).lines() {
        assert_eq!(size % 4, 0);
        let line = buf?;
        let mut elems = line.splitn(2, ",");
        let _ = elems.next().unwrap();
        let string = elems.next().unwrap();
        let string = string.replace("\\n", "\n");
        let (sjis, _, errors) = SHIFT_JIS.encode(&string);
        assert!(!errors);

        let mut line_size = sjis.len() + 1; // add null byte

        let diff = line_size % 4;
        if diff > 0 {
            line_size += 4 - diff;
        }

        size += line_size;
    }

    Ok(size as u32)
}
