use gw_types::{
    bytes::{BufMut, BytesMut},
    packed::RawL2Transaction,
};
use tokio::sync::mpsc;

#[derive(Debug)]
pub(crate) enum RedirLogMsg {
    Start(RawL2Transaction),
    Log(Vec<u8>),
    Flush(i8), //exit code
}
pub(crate) struct RedirLogActor {
    recv: mpsc::Receiver<RedirLogMsg>,
    ctx: Context,
}

impl RedirLogActor {
    pub(crate) fn new(recv: mpsc::Receiver<RedirLogMsg>) -> Self {
        let ctx = Context::init(None);
        Self { recv, ctx }
    }

    fn handle_msg(&mut self, msg: RedirLogMsg) {
        match msg {
            RedirLogMsg::Start(tx) => self.ctx.setup(tx),
            RedirLogMsg::Log(log) => self.ctx.append_log(&log),
            RedirLogMsg::Flush(exit_code) => self.ctx.flush(exit_code),
        }
    }
}

// We can store the whole context of contract execution with tx, logs and exit code.
struct Context {
    tx: Option<RawL2Transaction>,
    sink: BytesMut,
}

impl Context {
    fn init(tx: Option<RawL2Transaction>) -> Self {
        Self {
            tx,
            sink: BytesMut::with_capacity(1024),
        }
    }

    fn setup(&mut self, tx: RawL2Transaction) {
        self.tx = Some(tx);
    }

    fn append_log(&mut self, log: &[u8]) {
        self.sink.put(log);
        self.sink.put_u8(b'\n');
    }

    fn flush(&mut self, exit_code: i8) {
        if let Ok(s) = std::str::from_utf8(&self.sink) {
            log::debug!("[contract debug]: {}", s);
            log::debug!("contract exit code: {}", exit_code);
            //TODO
            //1. send to senty if exit code != 0
            //2. add more mode when print log, e.g only print log on error
        }
        self.sink.clear();
        self.tx = None;
    }
}

async fn run_redir_log_actor(mut actor: RedirLogActor) {
    while let Some(msg) = actor.recv.recv().await {
        actor.handle_msg(msg);
    }
}

#[derive(Clone)]
pub(crate) struct RedirLogHandler {
    sender: mpsc::Sender<RedirLogMsg>,
}

impl RedirLogHandler {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = mpsc::channel(16);
        let actor = RedirLogActor::new(receiver);
        tokio::spawn(run_redir_log_actor(actor));
        Self { sender }
    }

    pub(crate) fn start(&self, tx: RawL2Transaction) {
        self.send_msg(RedirLogMsg::Start(tx));
    }

    pub(crate) fn flush(&self, exit_code: i8) {
        self.send_msg(RedirLogMsg::Flush(exit_code));
    }

    pub(crate) fn append_log(&self, log: Vec<u8>) {
        self.send_msg(RedirLogMsg::Log(log));
    }

    fn send_msg(&self, msg: RedirLogMsg) {
        match self.sender.try_send(msg) {
            Ok(_) => log::trace!("redir log msg was sent out."),
            Err(mpsc::error::TrySendError::Closed(msg)) => {
                log::warn!(
                    "Discard redir log msg due to channel was closed. msg: {:?}",
                    msg
                )
            }
            Err(mpsc::error::TrySendError::Full(msg)) => {
                log::warn!(
                    "Discard redir log msg due to channel is full. msg: {:?}",
                    msg
                )
            }
        }
    }
}
