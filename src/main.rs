use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    time::{sleep, Duration},
};

use tokio_graceful_shutdown_rewrite::BoxedError;

async fn counter(sender: mpsc::Sender<u32>) {
    let mut counter: u32 = 0;
    loop {
        sleep(Duration::from_secs(1)).await;
        counter += 1;
        sender.send(counter).await.unwrap();
    }
}

async fn handle_connection(mut socket: TcpStream) -> Result<(), BoxedError> {
    let mut buf = [0u8; 64];

    let (count_sender, count_receiver) = mpsc::channel(1);

    tokio::spawn(counter(count_sender));

    loop {
        let data = match socket.read(&mut buf).await {
            Ok(num_read) => {
                if num_read == 0 {
                    break;
                }
                &buf[..num_read]
            }
            Err(e) => todo!(),
        };
    }

    Ok(())
}

async fn echo_server() -> Result<(), BoxedError> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;

    loop {
        let (socket, _) = listener.accept().await?;
        handle_connection(socket).await;
    }
}

#[tokio::main]
async fn main() {
    echo_server().await;
}
