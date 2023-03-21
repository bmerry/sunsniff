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

impl PacketCodec for Codec {
    type Item = Option<Arc<Update<'static>>>;

    /// Decode a single packet
    fn decode(&mut self, packet: Packet<'_>) -> Self::Item {
        if let Ok(sliced) = SlicedPacket::from_ethernet(packet.data) {
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
                for (&offset, field) in OFFSETS.iter().zip(FIELDS.iter()) {
                    let bytes = &sliced.payload[offset..offset + 2];
                    let bytes = <&[u8; 2]>::try_from(bytes).unwrap();
                    let value = i16::from_be_bytes(*bytes);
                    let value = (value as f64) * field.scale + field.bias;
                    values.push(value);
                }
                let update = Update::new(dt.timestamp_nanos(), serial, FIELDS, values);
                return Some(Arc::new(update));
            }
        }
        None
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
