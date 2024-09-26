use async_std::task;
use futures::channel::oneshot;
use futures::{select, FutureExt};

#[async_std::main]
async fn main() {
    // kreiramo dva kanala
    let (tx1, rx1) = oneshot::channel();
    let (tx2, rx2) = oneshot::channel();

    task::spawn(async {
        let _ = tx1.send("one");
    });

    task::spawn(async {
        let _ = tx2.send("two");
    });

    // pokličemo metodo fuse
    let mut rx1 = rx1.fuse();
    let mut rx2 = rx2.fuse();

    // hkrati čakamo na oba kanala
    let out = select! {
        val = rx1 => {
            format!("rx1 completed first with {:?}", val.unwrap())
        },
        val = rx2 => {
            format!("rx2 completed first with {:?}", val.unwrap())
        }
    };

    println!("Got = {}", out);
}