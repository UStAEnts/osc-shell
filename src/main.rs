use dirs::home_dir;
use rosc::decoder::decode;
use rosc::{encoder, OscMessage, OscPacket, OscType};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::process::Command;
use std::{fs, str};
use threadpool::ThreadPool;
use valico::json_schema;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Configuration {
    bind: String,
    port: i32,
    commands: HashMap<String, String>,
}
impl Configuration {
    fn new(value: serde_json::Value) -> Configuration {
        serde_json::from_value(value).unwrap()
    }
}

const JSON_SCHEMA: &str = r#"{
    "$schema": "http://json-schema.org/draft-06/schema#",
    "$id": "http://json-schema.org/draft-06/schema#",
    "title": "Core schema meta-schema",
    "type": "object",
    "properties": {
        "commands": {
            "type": "object",
            "patternProperties": {
                "^.*$": {"type": "string"}
            },
            "additionalProperties": false
        },
        "port": {
            "type": "integer"
        },
        "bind": {
            "type": "string"
        }
    },
    "required": [
        "commands",
        "port",
        "bind"
    ],
    "default": {}
}"#;

fn send_to_address(target: SocketAddr, packet: OscPacket) {
    let reply = match encoder::encode(&packet) {
        Ok(a) => a,
        Err(e) => {
            println!(
                "  failed to serialise the response into an OSC packet due to an error: {:?}",
                e
            );
            return;
        }
    };

    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(a) => a,
        Err(e) => {
            println!(
                "  failed to bind udp socket to 0.0.0.0 with unknown port due to an error: {:?}",
                e
            );
            return;
        }
    };

    match socket.send_to(&reply[..], target) {
        Ok(a) => a,
        Err(e) => {
            println!(
                "  failed to send the OSC packet back over the socket due to an error {:?}",
                e
            );
            return;
        }
    };
}

fn message(config: Configuration, message: OscMessage, src: SocketAddr) {
    let command = config.commands.get(&message.addr);
    if !command.is_none() {
        let packet = OscPacket::Message(OscMessage {
            addr: "/error".to_string(),
            args: vec![
                OscType::String(message.addr),
                OscType::String("unknown command".to_string()),
            ],
        });

        send_to_address(src, packet);

        println!("  command was not recognised in the configuration");
        return;
    }
    let mut command: String = command.unwrap().clone();

    for (i, arg) in message.args.iter().enumerate() {
        let from = &format!("${}", i)[..];
        match arg {
            OscType::Int(v) => command = command.replace(from, &format!("\"{}\"", v)[..]),
            OscType::Float(v) => command = command.replace(from, &format!("\"{}\"", v)[..]),
            OscType::String(v) => command = command.replace(from, &format!("\"{}\"", v)[..]),
            OscType::Time(v) => {
                command = command.replace(from, &format!("\"{}.{}\"", v.seconds, v.fractional)[..])
            }
            OscType::Long(v) => command = command.replace(from, &format!("\"{}\"", v)[..]),
            OscType::Double(v) => command = command.replace(from, &format!("\"{}\"", v)[..]),
            OscType::Char(v) => command = command.replace(from, &format!("\"{}\"", v)[..]),
            OscType::Color(v) => {
                command = command.replace(
                    from,
                    &format!("\"rgba({}, {}, {}, {})\"", v.red, v.green, v.blue, v.alpha)[..],
                )
            }
            OscType::Bool(v) => command = command.replace(from, &format!("\"{}\"", v)[..]),
            OscType::Nil => command = command.replace(from, "null"),
            OscType::Inf => command = command.replace(from, "inf"),
            // Not implementing Blob, Midi, Array
            _ => {
                println!("Warn: not replacing value due to being unsupported");
            }
        }
    }

    let command_split = match shell_words::split(&command[..]) {
        Ok(a) => a,
        Err(e) => {
            println!("  command in configuration was not valid after replacement - failed to split with a parse error: {:?}", e);
            return;
        }
    };

    println!("  executing => {:?}", command_split);

    let exec = Command::new(command_split[0].clone())
        .args(command_split[1..].to_vec())
        .output();

    match exec {
        Ok(a) => {
            let packet = OscPacket::Message(OscMessage {
                addr: "/success".to_string(),
                args: vec![
                    OscType::String(message.addr),
                    OscType::String(
                        str::from_utf8(&a.stdout[..])
                            .unwrap_or("cannot display output due to an encoding error")
                            .trim()
                            .to_string(),
                    ),
                ],
            });
            send_to_address(src, packet);
        }
        Err(e) => {
            let packet = OscPacket::Message(OscMessage {
                addr: "/error".to_string(),
                args: vec![
                    OscType::String(message.addr),
                    OscType::String("exec error".to_string()),
                ],
            });
            send_to_address(src, packet);
            println!("  failed due to an execution error: {:?}", e);
        }
    }
}

fn handle_osc(config: Configuration, packet: OscPacket, src: SocketAddr) {
    println!("OSC: {:?} {:?}", packet, src);
    match packet {
        OscPacket::Message(m) => message(config, m, src),
        OscPacket::Bundle(b) => {
            for m in b.content {
                handle_osc(config.clone(), m, src);
            }
        }
    }
}

fn handle_incoming(config: Configuration, data: &[u8], src: SocketAddr) {
    match decode(data) {
        Ok(packet) => {
            handle_osc(config, packet, src);
        }
        Err(e) => {
            println!(
                "Failed to parse incoming OSC message from {:?}: {:?}",
                src, e
            );
        }
    };
}

fn main() {
    // Build up where the file would be in the users home directory. This returns a PathBuf which is not super useful here but we can't convert
    // to a string in a one liner so we do that lower down
    let local_config = home_dir()
        .unwrap_or(PathBuf::from("~"))
        .join(".osc-commands.config.json");

    // Then compile the set of places the config could be in, from most important to least
    let config_locations = [
        "/etc/ents/osc-commands.json",
        // Convert to a string using a lossy conversion. This will swap out characters with the UTF replacement character if it fails. This function
        // returns a cow which isn't useful but we can't immediately deference it down because it turns into a str which has no size and therefore fails.
        // This needs to be &str but is a Cow so we have to dereference into a str and then borrow into an &str
        &*local_config.to_string_lossy(),
        "./config.json",
    ];

    // Try and load each of the configs in priority order, if they fail keep going. If all of them fail then it will be Option::None which is handled below
    let mut config: Option<String> = Option::None;
    for entry in config_locations {
        config = match fs::read_to_string(entry) {
            Ok(a) => Option::Some(a),
            Err(_) => continue,
        }
    }

    // If the config wasn't loaded, panic out
    let config_data = match config {
        Option::Some(a) => a,
        Option::None => {
            println!("No valid config file found");
            return;
        }
    };

    // Try and parse it as JSON failing out if it isn't valid
    let config: Value = match serde_json::from_str(&config_data[..]) {
        Ok(a) => a,
        Err(e) => {
            println!(
                "Config file could not be loaded due to an error parsing the JSON: {:?}",
                e
            );
            return;
        }
    };
    // Load the internal JSON schema - this should never fail because it would show up in testing but I still want to handle
    // the error in case something slips through the cracks
    let comp = match serde_json::from_str(JSON_SCHEMA) {
        Ok(a) => a,
        Err(e) => {
            println!("The internal JSON schema failed to parse - this should not happen so please report this to the maintainer with this error: {:?}", e);
            return;
        }
    };
    // Then actually parse it as JSON Schema according to the specification
    let mut scope = json_schema::Scope::new();
    let schema = match scope.compile_and_return(comp, false) {
        Ok(a) => a,
        Err(e) => {
            println!("The internal JSON schema parsed but was not valid JsonSchema - this should not happen so please report this to the maintainer with this error: {:?}", e);
            return;
        }
    };

    // Try and validate the contents of the configuration file and fail if its not valid. The error is somewhat descriptive even
    // if its not pretty so that is just printed out
    let state = schema.validate(&config);
    if !state.is_valid() {
        println!("Invalid configuration file: {:?}", state);
        return;
    }

    // Cast it to our internal type now we know its valid
    let config = Configuration::new(config);

    // Then create the listening socket that we receive datagram request through
    let socket = match UdpSocket::bind(format!("{}:{}", config.bind, config.port)) {
        Ok(a) => a,
        Err(e) => {
            println!("Failed to bind a UDP Socket {:?}", e);
            return;
        }
    };

    // To avoid commands blocking all progress, we allocate 4 threads for them to run in.
    // There aren't really enough ents to be doing long running tasks 4 at a time
    let pool = ThreadPool::new(4);

    loop {
        // Arbitrary limit on message sizes - 64KB of OSC
        let mut buf = [0; 64 * 1024];

        // For each packet that is received successfully
        match socket.recv_from(&mut buf) {
            Ok((amt, src)) => {
                // Clone the config because it can't be owned here
                let clone_config = config.clone();

                // Then move all required values into the coroutine and execute it in the thread pool
                pool.execute(move || {
                    handle_incoming(clone_config, &mut buf[..amt], src);
                })
            }
            Err(e) => {
                println!("Failed to receive message on UDP port {:?}", e);
            }
        };
    }
}
