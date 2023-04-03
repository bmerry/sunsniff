/* Copyright 2022-2023 Bruce Merry
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

use chrono::{DateTime, LocalResult, NaiveDate};
use chrono_tz::Tz;
use etherparse::SlicedPacket;
use futures::prelude::*;
use log::{error, info};
use pcap::{Capture, Device, Packet, PacketCodec};
use serde::Deserialize;
use std::ops::Range;
use std::sync::Arc;

use crate::receiver::{Update, UpdateStream};

/// Expected length of the packet (TCP payload)
const MAGIC_LENGTH: usize = 292;
/// Expected first byte of the packet
const MAGIC_HEADER: u8 = 0xa5;
/// Offsets containing the inverter serial number
const SERIAL_RANGE: Range<usize> = 11..21;
/// Offset at which the timestamp is located
const DATETIME_OFFSET: usize = 37;

/// Structure corresponding to the `[pcap]` section of the configuration file.
/// It is constructed from the config file by serde.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PcapConfig {
    device: String,
    #[serde(default)]
    file: bool,
    filter: Option<String>,
    timezone: Tz,
}

struct Codec {
    pub tz: Tz,
}

/// Extract the timestamp from the packet.
///
/// The timestamp consists of YY-MM-DD HH:MM:SS in 6 one-byte fields, with
/// the year relative to 2000. It is in local time, so needs to be combined
/// with the timestamp.
///
/// If the timestamp is an invalid time, or is invalid or ambiguous for the
/// time zone, returns `None`.
fn parse_timestamp(payload: &[u8], tz: Tz) -> Option<DateTime<Tz>> {
    let dt = NaiveDate::from_ymd_opt(
        payload[DATETIME_OFFSET] as i32 + 2000,
        payload[DATETIME_OFFSET + 1] as u32,
        payload[DATETIME_OFFSET + 2] as u32,
    )?
    .and_hms_opt(
        payload[DATETIME_OFFSET + 3] as u32,
        payload[DATETIME_OFFSET + 4] as u32,
        payload[DATETIME_OFFSET + 5] as u32,
    )?
    .and_local_timezone(tz);
    match dt {
        LocalResult::Single(x) => Some(x),
        _ => None, // TODO: what to do with ambiguous times - try to guess based on history?
    }
}

impl Codec {
    fn decode_data(&self, packet_data: &[u8]) -> Option<Arc<Update<'static>>> {
        if let Ok(sliced) = SlicedPacket::from_ethernet(packet_data) {
            if sliced.payload.len() == MAGIC_LENGTH && sliced.payload[0] == MAGIC_HEADER {
                let dt = match parse_timestamp(sliced.payload, self.tz) {
                    Some(x) => x,
                    None => {
                        return None; // Parse error means it's probably not the packet we expected
                    }
                };
                let serial =
                    std::str::from_utf8(&sliced.payload[SERIAL_RANGE]).unwrap_or("unknown");
                info!(
                    "Received packet with timestamp {:?} for inverter {}",
                    dt, serial
                );
                let mut values = Vec::with_capacity(FIELDS.len());
                for (&offsets, field) in OFFSETS.iter().zip(FIELDS.iter()) {
                    let parts = offsets.iter().cloned().map(|offset| {
                        let bytes = &sliced.payload[offset..offset + 2];
                        let bytes = <&[u8; 2]>::try_from(bytes).unwrap();
                        u16::from_be_bytes(*bytes)
                    });
                    let value = field.from_u16s(parts);
                    values.push(value);
                }
                let update = Update::new(dt.timestamp_nanos(), serial, FIELDS, values);
                return Some(Arc::new(update));
            }
        }
        None
    }
}

impl PacketCodec for Codec {
    type Item = Option<Arc<Update<'static>>>;

    /// Decode a single packet
    fn decode(&mut self, packet: Packet<'_>) -> Self::Item {
        self.decode_data(packet.data)
    }
}

async fn filter_fn(
    item: Result<Option<Arc<Update<'static>>>, pcap::Error>,
) -> Option<Arc<Update<'static>>> {
    match item {
        Ok(value) => value,
        Err(err) => {
            error!("Error from pcap: {err:?}");
            None
        }
    }
}

pub fn create_stream(config: &PcapConfig) -> Result<UpdateStream, Box<dyn std::error::Error>> {
    let base_filter = "tcp";
    let filter = match &config.filter {
        Some(expr) => format!("({}) and ({})", base_filter, expr),
        None => String::from(base_filter),
    };

    let codec = Codec {
        tz: config.timezone,
    };
    if config.file {
        let mut cap = Capture::from_file(&config.device)?;
        cap.filter(filter.as_str(), true)?;
        cap.set_datalink(pcap::Linktype::ETHERNET)?;
        /* cap.stream doesn't work on files. This is a somewhat hacky
         * workaround: it's probably going to load all the packets into
         * the sinks at once before giving them a chance to run.
         */
        Ok(Box::pin(
            futures::stream::iter(cap.iter(codec)).filter_map(filter_fn),
        ))
    } else {
        let device = Device::from(config.device.as_str());
        let cap = Capture::from_device(device)?.immediate_mode(true).open()?;
        let mut cap = cap.setnonblock()?;
        cap.filter(filter.as_str(), true)?;
        cap.set_datalink(pcap::Linktype::ETHERNET)?;
        Ok(Box::pin(cap.stream(codec)?.filter_map(filter_fn)))
    }
}

include!(concat!(env!("OUT_DIR"), "/pcap_fields.rs"));

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_decode_packet() {
        // Sample data from a real packet, but with the serial number altered for privacy
        let packet_data = [
            0x04, 0x42, 0x1a, 0x78, 0xac, 0xd0, 0x60, 0x55, 0xf9, 0xb0, 0x92, 0x14, 0x08, 0x00,
            0x45, 0x00, 0x01, 0x4c, 0x04, 0xf5, 0x00, 0x00, 0xff, 0x06, 0x80, 0x75, 0xc0, 0xa8,
            0x00, 0xca, 0x2f, 0xf2, 0x43, 0xdd, 0xc5, 0x9a, 0xc7, 0x9c, 0x67, 0x56, 0xe9, 0xb1,
            0x8d, 0xea, 0x57, 0xed, 0x50, 0x18, 0x15, 0xb6, 0xd3, 0x84, 0x00, 0x00, 0xa5, 0x06,
            0x01, 0x09, 0x02, 0xce, 0x00, 0x00, 0xfa, 0x01, 0x19, 0x31, 0x32, 0x33, 0x35, 0x36,
            0x38, 0x37, 0x31, 0x30, 0x38, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x16, 0x0b, 0x05, 0x08, 0x20, 0x2e, 0x01,
            0x00, 0x02, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x09, 0x7a, 0x00, 0x00, 0x01, 0x29,
            0x01, 0x13, 0x00, 0xc8, 0x0d, 0x1d, 0x00, 0x00, 0x00, 0x03, 0x00, 0x08, 0x08, 0x4a,
            0x00, 0x00, 0x05, 0x52, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x02, 0xe7, 0x13, 0x7a,
            0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0f, 0x0c, 0x5f, 0x00, 0x00,
            0x0a, 0xe1, 0x00, 0x00, 0x00, 0x00, 0x06, 0x30, 0x05, 0x9f, 0x00, 0x00, 0x00, 0x01,
            0x07, 0xd0, 0x00, 0x00, 0x0d, 0xfa, 0x00, 0x00, 0x08, 0x3e, 0x00, 0x00, 0x0a, 0x01,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x64,
            0x00, 0x07, 0x06, 0x65, 0x00, 0x39, 0x00, 0x4c, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x9e, 0x00, 0x01, 0xa2, 0x00, 0x01,
            0xcf, 0x5e, 0x21, 0xc1, 0x00, 0x2b, 0x09, 0x1d, 0x00, 0x00, 0x09, 0x1d, 0x00, 0x00,
            0x09, 0x1d, 0x00, 0x00, 0x09, 0x1d, 0x09, 0x4b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x84,
            0x00, 0x00, 0x01, 0x4d, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0xff, 0xb8,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xe6, 0x00, 0x00,
            0x00, 0xe6, 0x00, 0xe6, 0x00, 0x00, 0x00, 0xe6, 0x00, 0x9e, 0x00, 0x00, 0x00, 0x7e,
            0x04, 0xba, 0x14, 0xdf, 0x00, 0x36, 0x00, 0x9e, 0x03, 0xa2, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0xfd, 0x81, 0xfb, 0x54, 0x13, 0x7a, 0x13, 0x7a, 0x00, 0x01, 0x00, 0x10,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x15, 0xea, 0x00, 0x00, 0x00, 0x64,
            0x00, 0x69, 0x00, 0x36, 0x14, 0xda, 0x00, 0x0a, 0x04, 0xba,
        ];

        let c = Codec {
            tz: chrono_tz::Africa::Johannesburg,
        };
        let update = c.decode_data(&packet_data).unwrap();
        assert_eq!(update.serial, "1235687108");
        assert_eq!(update.timestamp, 1667629966000000000);
        let mut values = HashMap::<&str, f64>::new();
        for (field, value) in update.fields.iter().zip(update.values.iter()) {
            values.insert(field.id, *value);
        }
        // Just a smattering of values for sanity checking. This is not
        // intended to verify all the offsets.
        assert_eq!(values["grid_voltage"], 233.3);
        assert_eq!(values["battery_temperature"], 21.0);
        assert_eq!(values["battery_soc"], 54.0);
    }
}
