use std::{
    sync::{OnceLock, mpsc},
    thread,
};

static ASYNC_LOG_SENDER: OnceLock<mpsc::Sender<String>> = OnceLock::new();

pub fn async_log(message: impl Into<String>) {
    let sender = ASYNC_LOG_SENDER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<String>();

        let _ = thread::Builder::new()
            .name("async-log".to_string())
            .spawn(move || {
                while let Ok(message) = receiver.recv() {
                    println!("{message}");
                }
            });

        sender
    });

    let _ = sender.send(message.into());
}

#[macro_export]
macro_rules! async_log {
    ($($arg:tt)*) => {
        $crate::async_log::async_log(format!($($arg)*))
    };
}
