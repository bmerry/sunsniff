/* Copyright 2023-2024 Bruce Merry
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

use csv::StringRecord;
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::rc::Rc;

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
struct Metadata {
    field_type: FieldType,
    group: String,
    name: String,
    id: String,
    scale: Option<f64>,
}

struct Record {
    metadata: Rc<Metadata>,
    fields: Vec<i32>,
}

impl Record {
    fn new(
        metadata: &Rc<Metadata>,
        name1: &str,
        name2: &str,
        header_index: &HashMap<&str, usize>,
        row: &StringRecord,
    ) -> Option<Self> {
        let pos1 = header_index[name1];
        let pos2 = header_index[name2];
        let mut fields: Vec<i32> = vec![];
        if let Some(value1) = row.get(pos1).and_then(|x| x.parse().ok()) {
            if value1 >= 0 {
                fields.push(value1);
                if let Some(value2) = row.get(pos2).and_then(|x| x.parse().ok()) {
                    fields.push(value2);
                }
            }
            Some(Self {
                metadata: metadata.clone(),
                fields,
            })
        } else {
            None
        }
    }
}

fn write_fields<W>(w: &mut W, header: &str, records: &[Record]) -> Result<(), Box<dyn Error>>
where
    W: Write,
{
    writeln!(w, "use crate::fields::{{Field, FieldType}};")?;
    writeln!(w, "{header}")?;
    writeln!(w, "const FIELDS: &[Field] = &[")?;
    for record in records.iter() {
        let metadata = &record.metadata;
        let default_scale = match metadata.field_type {
            Charge | Power | StateOfCharge | Unitless => Some(1.0),
            Energy | Temperature => Some(0.1),
            Frequency => Some(0.01),
            Current | Voltage => None,
            Time => Some(60.0),
        };
        let bias = match metadata.field_type {
            Temperature => -100.0,
            _ => 0.0,
        };
        let unit = match metadata.field_type {
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
        let scale = metadata.scale.or(default_scale).unwrap();
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
            metadata.field_type, metadata.group, metadata.name, metadata.id
        )?;
    }
    writeln!(w, "];")?;

    writeln!(w, "#[allow(dead_code)]")?;
    writeln!(w, "mod field_idx {{")?;
    for (i, record) in records.iter().enumerate() {
        writeln!(
            w,
            "    pub const {}: usize = {};",
            record.metadata.id.to_uppercase(),
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
    let mut header_index = HashMap::new();
    let headers = reader.headers()?.clone();
    for (i, header) in headers.iter().enumerate() {
        header_index.insert(header, i);
    }

    let pcap_sizes = &[292, 302];
    let mut pcap_records = HashMap::new();
    for size in pcap_sizes {
        pcap_records.insert(size, vec![]);
    }
    let mut modbus_records = vec![];
    for row in reader.records() {
        let row = row?;
        let metadata: Metadata = row.deserialize(Some(&headers))?;
        let metadata = Rc::new(metadata);
        for (size, records) in pcap_records.iter_mut() {
            let offset_name = format!("v{size}_offset");
            let offset2_name = format!("v{size}_offset2");
            if let Some(record) =
                Record::new(&metadata, &offset_name, &offset2_name, &header_index, &row)
            {
                records.push(record);
            }
        }
        if let Some(record) = Record::new(&metadata, "reg", "reg2", &header_index, &row) {
            modbus_records.push(record);
        }
    }

    let mut pcap_writer = fs::File::create(pcap_path)?;
    write_fields(
        &mut pcap_writer,
        "/// Fields found in each packet",
        &pcap_records[&292],
    )?;
    writeln!(&mut pcap_writer, "/// Offsets of fields within packets")?;
    writeln!(&mut pcap_writer, "const OFFSETS: &[&[usize]] = &[")?;
    for record in pcap_records[&292].iter() {
        writeln!(&mut pcap_writer, "    &{:?},", record.fields.as_slice())?;
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
    for record in modbus_records.into_iter() {
        writeln!(&mut modbus_writer, "    &{:?},", record.fields.as_slice())?;
    }
    writeln!(&mut modbus_writer, "];")?;
    drop(modbus_writer);

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=fields.csv");
    Ok(())
}
