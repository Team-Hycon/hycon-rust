use crate::server::socket_parser::SocketParser;
use crate::server::Encode;
use bytes::{BufMut, Bytes, BytesMut};
use futures::future::{self, Either};
use futures::stream::{self, Stream};
use futures::sync::mpsc;
use std::collections::HashMap;
use std::io::{BufReader, Error, ErrorKind};
use std::iter;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio_core::reactor::Core;
use tokio_io::codec::Decoder;
use tokio_io::AsyncRead;

use crate::server::network_manager::{NetworkManager, NetworkMessage};

type Tx = mpsc::UnboundedSender<Bytes>;
type Rx = mpsc::UnboundedReceiver<Bytes>;

enum PeerStatus {
    Disconnected,
    Connected(crate::serialization::network::Status),
}
struct Peer {
    addr: SocketAddr,
    srv: Arc<Mutex<Server>>,
    socket: BaseSocket,
    receiver: Rx,
    status: PeerStatus,
}

impl Peer {
    fn new(
        srv: Arc<Mutex<Server>>,
        socket: BaseSocket,
        status: crate::serialization::network::Status,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded();
        let addr = socket.socket.peer_addr().unwrap();
        srv.lock().unwrap().peers.insert(addr, tx);
        Self {
            addr,
            srv,
            socket,
            receiver: rx,
            status: PeerStatus::Connected(status),
        }
    }

    fn return_status(&self) -> crate::serialization::network::Status {
        let mut status = crate::serialization::network::Status::new();
        status.set_guid(self.srv.lock().unwrap().guid.clone());
        status.set_version(self.srv.lock().unwrap().version.clone());
        status.set_port(3553);
        status.set_publicPort(3553);
        status.set_networkid("ouroboros".to_string());
        status
    }
}

impl Future for Peer {
    type Item = ();
    type Error = io::Error;
    fn poll(&mut self) -> Poll<(), Error> {
        match self.receiver.poll().unwrap() {
            Async::Ready(Some(v)) => {
                self.socket.buffer(&v);
                task::current().notify();
            }
            _ => {}
        }
        let _ = self.socket.poll_flush()?;
        while let Async::Ready(data) = self.socket.poll()? {
            if let Some(bytes) = data {
                if let Some(message) = self.socket.parser.parse(&bytes.to_vec()).unwrap() {
                    let mut msg = BytesMut::from(message);
                    println!("Message: {:?}", msg);
                    let msg = msg.freeze();
                    for (addr, tx) in &self.srv.lock().unwrap().peers {
                        if *addr != self.addr {
                            tx.unbounded_send(msg.clone()).unwrap();
                        }
                    }
                } else {
                    return Ok(Async::Ready(()));
                }
            }
        }
        Ok(Async::NotReady)
    }
}
struct Server {
    peers: HashMap<SocketAddr, Tx>,
    guid: String,
    version: u32,
}

impl Server {
    fn new() -> Self {
        Self {
            peers: HashMap::new(),
            guid: String::from("MyGuidIsGuid"),
            version: 14,
        }
    }
}

struct BaseSocket {
    socket: TcpStream,
    rd: BytesMut,
    wr: BytesMut,
    parser: SocketParser,
}
impl BaseSocket {
    fn new(socket: TcpStream) -> Self {
        Self {
            socket,
            rd: BytesMut::new(),
            wr: BytesMut::new(),
            parser: SocketParser::new(),
        }
    }
    fn buffer(&mut self, line: &[u8]) {
        self.wr.reserve(line.len());
        self.wr.put(line)
    }

    fn poll_flush(&mut self) -> Poll<(), io::Error> {
        while !self.wr.is_empty() {
            let n = try_ready!(self.socket.poll_write(&self.wr));
            assert!(n > 0);
            let _ = self.wr.split_to(n);
        }
        Ok(Async::Ready(()))
    }

    fn fill_read_buf(&mut self) -> Poll<(), io::Error> {
        loop {
            self.rd.reserve(1024);
            let n = try_ready!(self.socket.read_buf(&mut self.rd));
            if n == 0 {
                return Ok(Async::Ready(()));
            }
        }
    }
}

impl Stream for BaseSocket {
    type Item = BytesMut;
    type Error = std::io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let sock_closed = self.fill_read_buf()?.is_ready();
        if let Some(buf) = self.parser.parse(&mut self.rd.to_vec()).unwrap() {
            println!("Buffer: {:?}", buf);
            self.rd.clear();
            return Ok(Async::Ready(Some(BytesMut::from(buf))));
        }
        // let pos = self
        //     .rd
        //     .windows(2)
        //     .enumerate()
        //     .find(|&(_, bytes)| bytes == b"\n")
        //     .map(|(i, _)| i);
        // println!("Pos: {:?}", pos);
        // if let Some(pos) = pos {
        //     let mut line = self.rd.split_to(pos + 2);
        //     line.split_off(pos);
        //     return Ok(Async::Ready(Some(line)));
        // }

        if sock_closed {
            Ok(Async::Ready(None))
        } else {
            Ok(Async::NotReady)
        }
    }
}

fn process_socket(socket: TcpStream, server: Arc<Mutex<Server>>) {
    let base = BaseSocket::new(socket);
    let connection = base
        .into_future()
        .map_err(|(e, _)| e)
        .and_then(move |(message, base)| {
            if let Some(msg) = message {
                let parsed = NetworkManager::decode(&msg.to_vec()).unwrap();
                match &parsed.message_type {
                    crate::serialization::network::Network_oneof_request::status(v) => {
                        let mut peer = Peer::new(server, base, v.clone());
                        let status = peer.return_status();
                        let mut status_return = crate::serialization::network::StatusReturn::new();
                        status_return.set_status(status);
                        let net_msg = NetworkMessage::new(
                            crate::serialization::network::Network_oneof_request::statusReturn(
                                status_return,
                            ),
                        );
                        let bytes = peer
                            .socket
                            .parser
                            .prepare_packet(0, &net_msg.encode().unwrap());
                        match bytes {
                            Ok(msg) => {
                                println!("Message: {:?}", &msg);
                                peer.socket.buffer(&msg);
                            }
                            Err(e) => println!("Error: {}", e),
                        }
                        return Either::B(peer);
                    }
                    _ => {}
                }
                return Either::A(future::ok(()));
            } else {
                return Either::A(future::ok(()));
            }
        })
        .map_err(|e| {
            println!("Connection Error {:?}", e);
        });
    tokio::spawn(connection);
}
pub fn main(args: Vec<String>) -> Result<(), Box<std::io::Error>> {
    // let args: Vec<String> = ::std::env::args().collect();
    println!("Args: {:?}", args);
    let addr = args[2]
        .to_socket_addrs()
        .unwrap()
        .next()
        .expect("could not parse address");
    let mut core = Core::new()?;
    let handle = core.handle();
    let socket = TcpListener::bind(&addr)?;
    println!("Server listening on: {}", addr);

    let server = Arc::new(Mutex::new(Server::new()));

    let srv = socket.incoming().for_each(move |stream| {
        stream.set_nodelay(true)?;
        let (reader, writer) = stream.split();
        let (tx, rx) = mpsc::unbounded();
        let tx1 = tx.clone();
        let parser = Arc::new(Mutex::new(SocketParser::new()));
        let parser_clone = parser.clone();
        let reader = BufReader::new(reader);
        let iter = stream::iter_ok::<_, Error>(iter::repeat(()));
        let socket_reader = iter.fold(reader, move |reader, _| {
            let tx_inner = tx1.clone();
            let p = parser_clone.clone();
            let line = io::read_to_end(reader, Vec::new());
            let line = line.and_then(|(reader, vec)| {
                if vec.len() == 0 {
                    Err(Error::new(ErrorKind::BrokenPipe, "Broken Pipe"))
                } else {
                    Ok((reader, vec))
                }
            });
            line.map(move |(reader, vec)| {
                let mut bytes = BytesMut::new();
                bytes.extend_from_slice(&vec);
                println!("Received: {:?}", &bytes.to_vec());
                let mut guard = p.lock();
                let socket_parser = guard.as_mut().unwrap();
                match socket_parser.parse(&bytes.to_vec()) {
                    Ok(parse_result) => {
                        if let Some(parsed) = parse_result {
                            let message = NetworkManager::decode(&parsed).unwrap();
                            let decoded = message.encode().unwrap();
                            tx_inner.unbounded_send(BytesMut::from(decoded));
                        }
                    }
                    Err(e) => println!("Error: {}", e),
                }
                drop(guard);
                reader
            })
        });
        // let client = rx.and_then(|msg: BytesMut|{
        //     let amt = io::write_all(writer, msg);
        //     let amt = amt.map(|(writer, _)| writer);
        //     amt.map_err(|_| ())
        // });
        let socket_writer = rx.fold(writer, |writer, msg: BytesMut| {
            let amt = io::write_all(writer, msg);
            let amt = amt.map(|(writer, _)| writer);
            amt.map_err(|_| ())
        });
        let socket_reader = socket_reader.map_err(|_| ());
        let connection = socket_reader.map(|_| ()).select(socket_writer.map(|_| ()));
        handle.spawn(connection.then(move |_| {
            // connections.borrow_mut().remove(&addr);
            // println!("Connection {} closed.", addr);
            Ok(())
        }));
        Ok(())
    });
    Ok(core.run(srv).unwrap())
}

pub fn run(args: Vec<String>) -> Result<(), Box<std::error::Error>> {
    let srv = Arc::new(Mutex::new(Server::new()));
    let addr = args[2]
        .to_socket_addrs()
        .unwrap()
        .next()
        .expect("could not parse address");
    let socket = TcpListener::bind(&addr)?;
    let server = socket
        .incoming()
        .for_each(move |socket| {
            process_socket(socket, srv.clone());
            Ok(())
        })
        .map_err(|err| {
            println!("Accept error = {:?}", err);
        });
    println!("Server Running on {:?}", &addr);
    tokio::run(server);
    Ok(())
}
