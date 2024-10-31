use std::collections::HashMap;
use std::error::Error;
use std::future::Future;
use std::io::Cursor;
use bytes::{Buf, BytesMut};
use async_std::{prelude::*, task};
use async_std::net::{TcpListener, TcpStream};
use async_std::task::JoinHandle;
use futures::channel::mpsc;
use futures::sink::SinkExt;
use futures::{select, FutureExt, StreamExt};
use uuid::Uuid;
use websockets::{FragmentedMessage, Frame, Request, VecExt, StatusCode};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
type ResponseFrame = Vec<u8>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;
type Sender<T> = mpsc::UnboundedSender<T>;
type Users = HashMap<Uuid, Sender<ResponseFrame>>;

#[derive(Debug)]
enum Event {
    NewUser(Uuid, TcpStream),
    Message(ResponseFrame, Recipient),
}

#[derive(Debug)]
enum Recipient {
    All,
    User(Uuid),
}

#[async_std::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:7878").await?;
    println!("Listening on 127.0.0.1:7878");

    let (broker_tx, broker_rx) = mpsc::unbounded();
    task::spawn(async move {
        broker_loop(broker_rx).await.expect("Broker failure");
    });

    while let Some(Ok(stream)) = listener.incoming().next().await {
        spawn_and_log_error(handle_connection(stream, broker_tx.clone()));
    }

    Ok(())
}

fn spawn_and_log_error<F>(future: F) -> JoinHandle<()>
where
    F: Future<Output = Result<()>> + Send + 'static
{
    task::spawn(async move {
        if let Err(e) = future.await {
            eprintln!("{}", e)
        }
    })
}

async fn handle_connection(mut stream: TcpStream, mut broker_tx: Sender<Event>) -> Result<()> {
    let user = Uuid::new_v4();
    let mut buffer = BytesMut::zeroed(4096);

    // println!("Welcome user {:?}", user);

    if 0 == stream.read(&mut buffer).await? {
        return Err("Connection closed by remote.".into());
    }

    let request = Request::new(&buffer)?;
    if let Some(response) = request.response() {
        stream.write_all(response.as_bytes()).await?;
        stream.flush().await?;
        buffer.clear();
        let result = read_loop(user, stream, buffer, &mut broker_tx).await;
        if result.is_err() {
            let close = Frame::Close(StatusCode::ProtocolError);
            broker_tx.send(Event::Message(close.response().unwrap(), Recipient::User(user))).await.unwrap();
        }
        result
    } else {
        Err("Not a valid websocket request".into())
    }
}

async fn read_loop(
    user: Uuid,
    stream: TcpStream,
    mut buffer: BytesMut,
    broker_tx: &mut Sender<Event>
) -> Result<()> {
    let (rd, wr) = &mut (&stream, &stream);
    let wr = wr.clone();

    broker_tx.send(Event::NewUser(user, wr)).await.unwrap();

    let mut read_buffer = vec![0; 4096];
    let mut fragmented_message = FragmentedMessage::Text(vec![]);
    loop {
        let mut buff = Cursor::new(&buffer[..]);
        match Frame::parse(&mut buff, &mut fragmented_message) {
            Ok(frame) => {
                if let Some(response) = frame.response() {
                    if frame.is_text() || frame.is_binary() || frame.is_continuation() {
                        broker_tx.send(Event::Message(response, Recipient::All)).await.unwrap();
                        fragmented_message = FragmentedMessage::Text(vec![]);
                    } else {
                        broker_tx.send(Event::Message(response, Recipient::User(user))).await.unwrap();
                        if frame.is_close() {
                            return Ok(());
                        }
                    }
                }
                buffer.advance(buff.position() as usize);
            }
            Err(_) => {
                let n = rd.read(&mut read_buffer).await?;
                if n == 0 {
                    return Err("Connection closed by remote.".into());
                }
                buffer.extend_from_slice(&read_buffer[..n]);
            }
        }
    }
}

async fn writer_loop(mut user_rx: Receiver<ResponseFrame>, mut wr: TcpStream) -> Result<()> {
    while let Some(message) = user_rx.next().await {
        wr.write_all(&message).await?;
        wr.flush().await?;
        if message.is_close() {
            break;
        }
    }
    Ok(())
}

async fn broker_loop(broker_rx: Receiver<Event>) -> Result<()> {
    let (disconnect_tx, disconnect_rx) = mpsc::unbounded();
    let mut users= Users::new();

    let mut broker_rx = broker_rx.fuse();
    let mut disconnect_rx = disconnect_rx.fuse();
    loop {
        let event = select! {
            event = broker_rx.next().fuse() => match event {
                None => break,
                Some(event) => event,
            },
            user = disconnect_rx.next().fuse() => match user {
                None => break,
                Some(user) => {
                    // println!("Goodbye user: {:?}", user);
                    users.remove(&user);
                    continue;
                }
            }
        };
        match event {
            Event::NewUser(user, wr) => {
                let (user_tx, user_rx) = mpsc::unbounded();
                users.insert(user, user_tx);
                let mut disconnect_tx = disconnect_tx.clone();
                spawn_and_log_error(async move {
                    let result = writer_loop(user_rx, wr).await;
                    disconnect_tx.send(user).await.unwrap();
                    result
                });
            }
            Event::Message(message, recipient) => match recipient {
                Recipient::All => {
                    for user_tx in users.values_mut() {
                        if let Err(e) = user_tx.send(message.clone()).await {
                            eprintln!("Failed sending to other users: {}", e);
                        }
                    }
                }
                Recipient::User(user) => {
                    if let Some(user_tx) = users.get_mut(&user) {
                        user_tx.send(message).await.unwrap();
                    }
                }
            }
        }
    }

    Ok(())
}
