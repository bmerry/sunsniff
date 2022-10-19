use clap::Parser;
use etherparse::SlicedPacket;
use futures::channel::mpsc::UnboundedSender;
use futures::join;
use futures::stream::{FuturesUnordered, StreamExt};
use influxdb2::Client;
use pcap::{Capture, Device, Packet, PacketCodec};
use std::sync::Arc;

use sunsniff::influxdb2::Influxdb2Receiver;
use sunsniff::receiver::{Field, Receiver, Update};

#[derive(Debug, Parser)]
#[clap(author, version)]
struct Args {
    /// Host for influxdb
    #[clap(long, default_value = "http://localhost:8086")]
    host: String,

    /// Organisation for influxdb
    #[clap(long, required = true)]
    org: String,

    /// Token for influxdb
    #[clap(long, required = true)]
    token: String,

    /// Bucket for influxdb
    #[clap(long, required = true)]
    bucket: String,

    /// Capture device
    #[clap(long, required = true)]
    device: String,

    /// Treat --device as a file rather than a device
    #[clap(long)]
    file: bool,

    /// Filter expression for pcap
    #[clap(long)]
    filter: Option<String>,
}

const MAGIC_LENGTH: usize = 292;
const MAGIC_HEADER: u8 = 0xa5;
const FIELDS: &[Field] = &[
    Field::energy(70, "Battery", "Total charge"),
    Field::energy(74, "Battery", "Total discharge"),
    Field::energy(82, "Grid", "Total import"),
    Field::energy(88, "Grid", "Total export"),
    Field::frequency(84, "Grid"),
    Field::energy(96, "Load", "Total consumption"),
    Field::temperature_name(106, "Inverter", "DC Temperature"),
    Field::temperature_name(108, "Inverter", "AC Temperature"),
    Field::energy(118, "PV", "Total production"),
    Field::voltage(176, "Grid"),
    Field::power(216, "Grid"),
    Field::power(228, "Load"),
    Field::temperature(240, "Battery"),
    Field::new(244, "Battery", "SOC", 1.0, 0.0, "%"),
    Field::power(248, "PV"),
    Field::power(256, "Battery"),
    Field::current(258, "Battery"),
];

struct Codec {}

impl PacketCodec for Codec {
    type Item = Result<Option<Arc<Update<'static>>>, Box<dyn std::error::Error>>;

    fn decode(&mut self, packet: Packet<'_>) -> Self::Item {
        let sliced = SlicedPacket::from_ethernet(packet.data)?;
        if sliced.payload.len() == MAGIC_LENGTH && sliced.payload[0] == MAGIC_HEADER {
            let serial = std::str::from_utf8(&sliced.payload[11..21]).unwrap_or("unknown");
            let timestamp = (packet.header.ts.tv_sec as i64) * 1000000000i64
                + (packet.header.ts.tv_usec as i64) * 1000i64;
            let mut values = vec![];
            for field in FIELDS.iter() {
                let bytes = &sliced.payload[field.offset..field.offset + 2];
                let bytes = <&[u8; 2]>::try_from(bytes)?;
                let value = i16::from_be_bytes(*bytes);
                let value = (value as f64) * field.scale + field.bias;
                values.push(value);
            }
            let update = Update::new(timestamp, serial, FIELDS, values);
            return Ok(Some(Arc::new(update)));
        }
        Ok(None)
    }
}

async fn run<T: pcap::Activated>(
    mut cap: Capture<T>,
    args: &Args,
    sinks: &mut [UnboundedSender<Arc<Update<'static>>>],
) -> Result<(), Box<dyn std::error::Error>> {
    let base_filter = "tcp";
    let filter = match &args.filter {
        Some(expr) => format!("({}) and ({})", base_filter, expr),
        None => String::from(base_filter),
    };
    cap.filter(filter.as_str(), true)?;
    cap.set_datalink(pcap::Linktype::ETHERNET)?;
    let mut stream = cap.stream(Codec {})?;

    loop {
        match stream.next().await {
            Some(item) => {
                match item?? {
                    Some(update) => {
                        for sink in sinks.iter_mut() {
                            sink.unbounded_send(Arc::clone(&update))?;
                        }
                    }
                    None => {}
                }
            }
            None => { break; }
        }
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let client = Client::new(&args.host, &args.org, &args.token);
    // Check that we're at least able to connect to the server
    // TODO: actually check that the server is healthy
    client.health().await?;

    let receiver = Influxdb2Receiver::new(client, &args.bucket);
    let mut receivers = vec![receiver];

    let mut sinks = vec![];
    let futures = FuturesUnordered::new();
    for receiver in receivers.iter_mut() {
        let (sink, stream) = futures::channel::mpsc::unbounded();
        futures.push(receiver.run(stream));
        sinks.push(sink);
    }

    if args.file {
        let cap = Capture::from_file(&args.device)?;
        join!(run(cap, &args, &mut sinks), futures.collect::<Vec<_>>()).0?;
    } else {
        let device = Device::from(args.device.as_str());
        let cap = Capture::from_device(device)?.immediate_mode(true).open()?;
        join!(run(cap, &args, &mut sinks), futures.collect::<Vec<_>>()).0?;
    }
    Ok(())
}
