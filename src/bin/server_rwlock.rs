use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use async_std::{prelude::*, task};
use async_std::net::{TcpListener, TcpStream};
use async_std::sync::{RwLock};
use futures::channel::mpsc;
use futures::sink::SinkExt;
use uuid::Uuid;
use std::io::Cursor;
use bytes::{BytesMut, Buf};
use websockets::{Frame, FragmentedMessage, Request, VecExt};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
type ResponseFrame = Vec<u8>;
type Sender<T> = mpsc::UnboundedSender<T>;
type Users = Arc<RwLock<HashMap<Uuid, Sender<ResponseFrame>>>>;

#[async_std::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:7878").await?;
    println!("Listening on 127.0.0.1:7878");

    let users = Users::default();
    while let Some(stream) = listener.incoming().next().await {
        let stream = stream?;
        let users = users.clone();
        task::spawn(async move {
            handle_connection(stream, users).await.expect("Failure when handling connection");
        });
    }
    Ok(())
}

async fn handle_connection(mut stream: TcpStream, users: Users) -> Result<()> {
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
        handle_websocket_frames(users, user, stream, buffer).await
    } else {
        Err("Not a valid websocket request".into())
    }
}

async fn handle_websocket_frames(
    users: Users,
    user: Uuid,
    stream: TcpStream,
    buffer: BytesMut
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded();
    users.write().await.insert(user, tx);

    let (rd, wr) = &mut (&stream, &stream);

    let rd = rd.clone();
    let users_rd = users.clone();
    task::spawn(async move {
        if let Err(e) = read_loop(buffer, rd, &users_rd, user).await {
            println!("Error: {:?}", e);
            disconnect(user, &users_rd).await;
        }
    });

    while let Some(response) = rx.next().await {
        if let Err(e) = wr.write_all(&response).await {
            println!("Error: {:?}", e);
            disconnect(user, &users).await;
            break;
        }
        wr.flush().await.unwrap();
        if response.is_close() {
            disconnect(user, &users).await;
            break;
        }
    }

    Ok(())
}

async fn read_loop(
    mut buffer: BytesMut,
    mut rd: TcpStream,
    users: &Users,
    user: Uuid
) -> Result<()> {
    let mut read_buffer = vec![0; 4096];
    let mut fragmented_message = FragmentedMessage::Text(vec![]);
    loop {
        let mut buff = Cursor::new(&buffer[..]);
        match Frame::parse(&mut buff, &mut fragmented_message) {
            Ok(frame) => {
                if let Some(response) = frame.response() {
                    if frame.is_text() || frame.is_binary() || frame.is_continuation() {
                        for tx in users.write().await.values_mut() {
                            tx.send(response.clone()).await.expect("Failed to send message");
                        }
                        fragmented_message = FragmentedMessage::Text(vec![]);
                    } else {
                        if let Some(tx) = users.write().await.get_mut(&user) {
                            tx.send(response).await.expect("Failed to send message");
                        }
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

async fn disconnect(user: Uuid, users: &Users) {
    // println!("Goodbye user: {:?}", user);
    users.write().await.remove(&user);
}

