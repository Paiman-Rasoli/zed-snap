//! Clipboard access on a dedicated thread. Keeping one `arboard::Clipboard`
//! alive for the whole server lifetime keeps the image available on Linux,
//! where clipboard contents are owned by the process that set them.

use std::borrow::Cow;
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;

use anyhow::{anyhow, Result};

struct Request {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
    reply: Sender<Result<(), String>>,
}

fn worker() -> &'static std::sync::Mutex<Sender<Request>> {
    static TX: OnceLock<std::sync::Mutex<Sender<Request>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = channel::<Request>();
        std::thread::spawn(move || {
            let mut clipboard = None;
            for req in rx {
                if clipboard.is_none() {
                    clipboard = arboard::Clipboard::new().ok();
                }
                let result = match clipboard.as_mut() {
                    Some(cb) => cb
                        .set_image(arboard::ImageData {
                            width: req.width,
                            height: req.height,
                            bytes: Cow::Owned(req.rgba),
                        })
                        .map_err(|e| e.to_string()),
                    None => Err("clipboard unavailable".into()),
                };
                if result.is_err() {
                    clipboard = None; // retry with a fresh handle next time
                }
                let _ = req.reply.send(result);
            }
        });
        std::sync::Mutex::new(tx)
    })
}

pub fn copy_image(width: u32, height: u32, rgba: Vec<u8>) -> Result<()> {
    let (reply, rx) = channel();
    worker()
        .lock()
        .map_err(|_| anyhow!("clipboard worker poisoned"))?
        .send(Request {
            width: width as usize,
            height: height as usize,
            rgba,
            reply,
        })
        .map_err(|_| anyhow!("clipboard worker stopped"))?;
    rx.recv()
        .map_err(|_| anyhow!("clipboard worker stopped"))?
        .map_err(|e| anyhow!(e))
}
