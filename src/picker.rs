//! 系统文件选择器 worker。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError};

use bevy::log::warn;

/// 文件选择器状态。
#[derive(bevy::prelude::Resource)]
pub struct Picker {
    receiver: Mutex<Receiver<Option<PathBuf>>>,
    open: bool,
}

impl Default for Picker {
    fn default() -> Self {
        Self::new()
    }
}

impl Picker {
    /// 创建空闲状态。
    #[must_use]
    pub fn new() -> Self {
        let (_sender, receiver) = std::sync::mpsc::channel();
        Self {
            receiver: Mutex::new(receiver),
            open: false,
        }
    }

    /// 打开系统文件选择器。
    pub fn open(&mut self, directory: Option<PathBuf>) {
        if self.open {
            return;
        }

        let (sender, receiver) = std::sync::mpsc::channel();
        if let Ok(mut guard) = self.receiver.lock() {
            *guard = receiver;
        }
        self.open = true;

        let result = std::thread::Builder::new()
            .name("bms-preview-picker".to_owned())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new()
                    .add_filter("BMS/BMSON", &["bms", "bme", "bml", "pms", "bmson"]);
                if let Some(directory) = directory {
                    dialog = dialog.set_directory(directory);
                }
                let _ = sender.send(dialog.pick_file());
            });

        if let Err(error) = result {
            warn!("无法启动文件选择器线程: {error}");
            self.open = false;
        }
    }

    /// 轮询选择结果；返回 `Some(None)` 表示用户取消。
    pub fn poll(&mut self) -> Option<Option<PathBuf>> {
        if !self.open {
            return None;
        }
        let receiver = self.receiver.lock().ok()?;
        match receiver.try_recv() {
            Ok(result) => {
                self.open = false;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.open = false;
                Some(None)
            }
        }
    }
}
