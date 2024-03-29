/* Copyright 2023 Bruce Merry
 *
 * This program is free software: you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the Free
 * Software Foundation, either version 3 of the License, or (at your option)
 * any later version.
 *
 * This program is distributed in the hope that it will be useful, but WITHOUT
 * ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
 * FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for
 * more details.
 *
 * You should have received a copy of the GNU General Public License along
 * with this program. If not, see <https://www.gnu.org/licenses/>.
 */

use serde::Deserialize;
use std::env;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Duplicate of crate::fields::FieldType
#[derive(Deserialize, Debug, Clone)]
enum FieldType {
    Charge,
    Current,
    Energy,
    Frequency,
    Power,
    StateOfCharge,
    Temperature,
    Time,
    Voltage,
    Unitless,
}

use FieldType::*;

#[derive(Deserialize, Clone)]
struct Record {
    field_type: FieldType,
    group: String,
    name: String,
    id: String,
    scale: Option<f64>,
    offset: Option<u32>,
    offset2: Option<u32>,
    reg: Option<i16>,
    reg2: Option<i16>,
}

fn write_fields<W>(w: &mut W, header: &str, records: &[Record]) -> Result<(), Box<dyn Error>>
where
    W: Write,
{
    writeln!(w, "use crate::fields::{{Field, FieldType}};")?;
    writeln!(w, "{header}")?;
    writeln!(w, "const FIELDS: &[Field] = &[")?;
    for record in records.iter() {
        let default_scale = match record.field_type {
            Charge | Power | StateOfCharge | Unitless => Some(1.0),
            Energy | Temperature => Some(0.1),
            Frequency => Some(0.01),
            Current | Voltage => None,
            Time => Some(60.0),
        };
        let bias = match record.field_type {
            Temperature => -100.0,
            _ => 0.0,
        };
        let unit = match record.field_type {
            Charge => "Ah",
            Current => "A",
            Energy => "kWh",
            Frequency => "Hz",
            Power => "W",
            StateOfCharge => "%",
            Temperature => "°C",
            Time => "s",
            Voltage => "V",
            Unitless => "",
        };
        let scale = record.scale.or(default_scale).unwrap();
        writeln!(
            w,
            r#"    Field {{
        field_type: FieldType::{:?},
        group: {:?},
        name: {:?},
        id: {:?},
        scale: {scale:?},
        bias: {bias:?},
        unit: {unit:?},
    }},"#,
            record.field_type, record.group, record.name, record.id
        )?;
    }
    writeln!(w, "];")?;

    writeln!(w, "#[allow(dead_code)]")?;
    writeln!(w, "mod field_idx {{")?;
    for (i, record) in records.iter().enumerate() {
        writeln!(
            w,
            "    pub const {}: usize = {};",
            record.id.to_uppercase(),
            i
        )?;
    }
    writeln!(w, "}}")?;

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir);
    let pcap_path = out_path.join("pcap_fields.rs");
    let modbus_path = out_path.join("modbus_fields.rs");
    let mut reader = csv::Reader::from_reader(fs::File::open("fields.csv")?);

    let mut pcap_records = vec![];
    let mut pcap_offsets = vec![];
    let mut modbus_records = vec![];
    let mut modbus_regs = vec![];
    for result in reader.deserialize() {
        let record: Record = result?;
        if let Some(offset) = record.offset {
            pcap_records.push(record.clone());
            let mut offsets = vec![offset];
            if let Some(offset2) = record.offset2 {
                offsets.push(offset2);
            }
            pcap_offsets.push(offsets);
        }
        if let Some(reg) = record.reg {
            modbus_records.push(record.clone());
            let mut regs = vec![];
            if reg >= 0 {
                regs.push(reg);
                if let Some(reg2) = record.reg2 {
                    regs.push(reg2);
                }
            }
            modbus_regs.push(regs);
        }
    }

    let mut pcap_writer = fs::File::create(pcap_path)?;
    write_fields(
        &mut pcap_writer,
        "/// Fields found in each packet",
        &pcap_records,
    )?;
    writeln!(&mut pcap_writer, "/// Offsets of fields within packets")?;
    writeln!(&mut pcap_writer, "const OFFSETS: &[&[usize]] = &[")?;
    for offsets in pcap_offsets.into_iter() {
        writeln!(&mut pcap_writer, "    &{:?},", offsets.as_slice())?;
    }
    writeln!(&mut pcap_writer, "];")?;
    drop(pcap_writer);

    let mut modbus_writer = fs::File::create(modbus_path)?;
    write_fields(
        &mut modbus_writer,
        "/// Fields retrieved by modbus protocol",
        &modbus_records,
    )?;
    writeln!(&mut modbus_writer, "/// Registers corresponding to fields")?;
    writeln!(&mut modbus_writer, "const REGISTERS: &[&[u16]] = &[")?;
    for regs in modbus_regs.into_iter() {
        writeln!(&mut modbus_writer, "    &{:?},", regs.as_slice())?;
    }
    writeln!(&mut modbus_writer, "];")?;
    drop(modbus_writer);

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=fields.csv");
    Ok(())
}
