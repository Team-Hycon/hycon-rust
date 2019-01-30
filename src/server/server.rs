use bytes::{BufMut, BytesMut};
use futures::stream::{self, Stream};
use futures::Future;
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::io::{BufReader, Error, ErrorKind};
use std::iter;
use std::net::ToSocketAddrs;
use std::rc::Rc;
use tokio_core::net::TcpListener;
use tokio_core::reactor::Core;
use tokio_io::io;
use tokio_io::AsyncRead;

use tokio_io::codec::Decoder;

use crate::protocol::protocol::Protocol;

pub fn main() {
    let args: Vec<String> = ::std::env::args().collect();
    let addr = args[2]
        .to_socket_addrs()
        .unwrap()
        .next()
        .expect("could not parse address");
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let socket = TcpListener::bind(&addr, &handle).unwrap();
    println!("Server listening on: {}", addr);

    let connections = Rc::new(RefCell::new(HashMap::new()));

    let srv = socket.incoming().for_each(move |(stream, addr)| {
        println!("New Connection: {}", addr);
        let (reader, writer) = stream.split();

        let (tx, rx) = futures::sync::mpsc::unbounded();
        connections.borrow_mut().insert(addr, tx);

        let connections_inner = connections.clone();
        let reader = BufReader::new(reader);

        let iter = stream::iter_ok::<_, Error>(iter::repeat(()));
        let socket_reader = iter.fold(reader, move |reader, _| {
            let line = io::read_until(reader, b'\n', Vec::new());
            let line = line.and_then(|(reader, vec)| {
                if vec.len() == 0 {
                    Err(Error::new(ErrorKind::BrokenPipe, "Broken Pipe"))
                } else {
                    Ok((reader, vec))
                }
            });
            let mut bytes = BytesMut::new();
            line.map(move |(reader, vec)| {
                bytes.extend_from_slice(&vec);
                let mut proto = Protocol::new();
                let decoded = proto.decode(&mut bytes).unwrap();
                match decoded {
                    Some(msg) => proto = msg,
                    None => (),
                }
                println!("Message: {}", &proto.msg);
                reader
            })
        });

        let socket_writer = rx.fold(writer, |writer, msg: BytesMut| {
            let amt = io::write_all(writer, msg);
            let amt = amt.map(|(writer, _)| writer);
            amt.map_err(|_| ())
        });
        Ok(())
    });
    core.run(srv).unwrap();
}