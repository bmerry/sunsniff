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
use serde::{Deserialize, Deserializer};
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

fn split_str<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    Ok(s.split(' ')
        .map(|x| x.to_owned())
        .filter(|x| !x.is_empty())
        .collect())
}

#[derive(Deserialize, Clone)]
struct Field {
    field_type: FieldType,
    group: String,
    name: String,
    id: String,
    scale: Option<f64>,
    #[serde(deserialize_with = "split_str")]
    sum_of: Vec<String>,
}

struct Record {
    field: Rc<Field>,
    positions: Vec<i32>,
}

impl Record {
    fn new(
        field: &Rc<Field>,
        name1: &str,
        name2: &str,
        header_index: &HashMap<&str, usize>,
        row: &StringRecord,
    ) -> Option<Self> {
        let col1 = header_index[name1];
        let col2 = header_index[name2];
        let mut positions: Vec<i32> = vec![];
        if let Some(value1) = row.get(col1).and_then(|x| x.parse().ok()) {
            if value1 >= 0 {
                positions.push(value1);
                if let Some(value2) = row.get(col2).and_then(|x| x.parse().ok()) {
                    positions.push(value2);
                }
            }
            Some(Self {
                field: field.clone(),
                positions,
            })
        } else {
            None
        }
    }
}

fn write_fields_data<W>(w: &mut W, records: &[Record]) -> Result<(), Box<dyn Error>>
where
    W: Write,
{
    writeln!(w, "&[")?;
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    for (i, record) in records.iter().enumerate() {
        let field = &record.field;
        let default_scale = match field.field_type {
            Charge | Power | StateOfCharge | Unitless => Some(1.0),
            Energy | Temperature => Some(0.1),
            Frequency => Some(0.01),
            Current | Voltage => None,
            Time => Some(60.0),
        };
        let bias = match field.field_type {
            Temperature => -100.0,
            _ => 0.0,
        };
        let unit = match field.field_type {
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
        let scale = field.scale.or(default_scale).unwrap();
        let sum_of: Vec<usize> = field
            .sum_of
            .iter()
            .map(|id| {
                *by_id
                    .get(id.as_str())
                    .unwrap_or_else(|| panic!("Prior field {id:?} not found"))
            })
            .collect();
        writeln!(
            w,
            r#"    crate::fields::Field {{
        field_type: crate::fields::FieldType::{:?},
        group: {:?},
        name: {:?},
        id: {:?},
        scale: {scale:?},
        bias: {bias:?},
        unit: {unit:?},
        sum_of: &{:?},
    }},"#,
            field.field_type,
            field.group,
            field.name,
            field.id,
            sum_of.as_slice()
        )?;
        by_id.insert(field.id.as_str(), i);
    }
    write!(w, "]")?;
    Ok(())
}

fn write_fields<W>(w: &mut W, header: &str, records: &[Record]) -> Result<(), Box<dyn Error>>
where
    W: Write,
{
    writeln!(w, "{header}")?;
    write!(w, "const FIELDS: &[crate::fields::Field] = ")?;
    write_fields_data(w, records)?;
    writeln!(w, ";")?;

    writeln!(w, "#[allow(dead_code)]")?;
    writeln!(w, "mod field_idx {{")?;
    for (i, record) in records.iter().enumerate() {
        writeln!(
            w,
            "    pub const {}: usize = {};",
            record.field.id.to_uppercase(),
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

    let pcap_sizes: &[usize] = &[292, 302];
    let mut pcap_records = HashMap::new();
    for size in pcap_sizes {
        pcap_records.insert(size, vec![]);
    }
    let mut modbus_records = vec![];
    for row in reader.records() {
        let row = row?;
        let field: Field = row.deserialize(Some(&headers))?;
        let field = Rc::new(field);
        for (size, records) in pcap_records.iter_mut() {
            let offset_name = format!("v{size}_offset");
            let offset2_name = format!("v{size}_offset2");
            if let Some(record) =
                Record::new(&field, &offset_name, &offset2_name, &header_index, &row)
            {
                records.push(record);
            }
        }
        if let Some(record) = Record::new(&field, "reg", "reg2", &header_index, &row) {
            modbus_records.push(record);
        }
    }

    {
        let mut pcap_writer = fs::File::create(pcap_path)?;
        let mut builder = phf_codegen::Map::new();
        for (size, records) in pcap_records.iter() {
            let mut buf = Vec::new();
            writeln!(&mut buf, "    FieldOffsets {{")?;
            write!(&mut buf, "        fields: ")?;
            write_fields_data(&mut buf, records)?;
            writeln!(&mut buf, ",")?;
            write!(&mut buf, "        offsets: &[")?;
            for record in records.iter() {
                writeln!(&mut buf, "            &{:?},", record.positions.as_slice())?;
            }
            writeln!(&mut buf, "        ],")?;
            writeln!(&mut buf, "    }}")?;
            builder.entry(size, String::from_utf8(buf)?);
        }
        writeln!(&mut pcap_writer, "struct FieldOffsets {{")?;
        writeln!(
            &mut pcap_writer,
            "    fields: &'static [crate::fields::Field<'static>],"
        )?;
        writeln!(
            &mut pcap_writer,
            "    offsets: &'static [&'static [usize]],"
        )?;
        writeln!(&mut pcap_writer, "}}")?;
        writeln!(
            &mut pcap_writer,
            "/// Field definitions for different packet sizes"
        )?;
        writeln!(
            &mut pcap_writer,
            "static FIELDS: phf::Map<usize, FieldOffsets> = {};",
            builder.build()
        )?;
    }

    {
        let mut modbus_writer = fs::File::create(modbus_path)?;
        write_fields(
            &mut modbus_writer,
            "/// Fields retrieved by modbus protocol",
            &modbus_records,
        )?;
        writeln!(&mut modbus_writer, "/// Registers corresponding to fields")?;
        writeln!(&mut modbus_writer, "const REGISTERS: &[&[u16]] = &[")?;
        for record in modbus_records.into_iter() {
            writeln!(
                &mut modbus_writer,
                "    &{:?},",
                record.positions.as_slice()
            )?;
        }
        writeln!(&mut modbus_writer, "];")?;
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=fields.csv");
    Ok(())
}
