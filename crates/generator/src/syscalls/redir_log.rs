use gw_config::ContractLogConfig;
use gw_types::packed::RawL2Transaction;
use tokio::sync::mpsc;

#[derive(Debug)]
pub(crate) enum RedirLogMsg {
    Session(String), // tx hash
    Log(String),
    Flush(i8), //exit code
}
pub(crate) struct RedirLogActor {
    recv: mpsc::Receiver<RedirLogMsg>,
    ctx: Context,
}

impl RedirLogActor {
    pub(crate) fn new(recv: mpsc::Receiver<RedirLogMsg>, config: ContractLogConfig) -> Self {
        let ctx = Context::init(config);
        Self { recv, ctx }
    }

    fn handle_msg(&mut self, msg: RedirLogMsg) {
        match msg {
            RedirLogMsg::Session(tx_hash) => self.ctx.setup(tx_hash),
            RedirLogMsg::Log(log) => self.ctx.append_log(log),
            RedirLogMsg::Flush(exit_code) => self.ctx.flush(exit_code),
        }
    }
}

// We can store the whole context of contract execution with tx, logs and exit code.
struct Context {
    buf: String,
    config: ContractLogConfig,
}

impl Context {
    fn init(config: ContractLogConfig) -> Self {
        Self {
            buf: String::with_capacity(1024),
            config,
        }
    }

    fn setup(&mut self, tx_hash: String) {
        self.buf.clear();
        self.buf.push_str(&tx_hash);
        self.buf.push('\n');
    }

    fn append_log(&mut self, log: String) {
        self.buf.push_str(&log);
        self.buf.push('\n');
    }

    fn flush(&mut self, exit_code: i8) {
        self.buf.push_str("exit code: ");
        self.buf.push_str(&exit_code.to_string());
        if self.config == ContractLogConfig::Verbose
            || (self.config == ContractLogConfig::Error && exit_code != 0)
        {
            log::debug!("[contract debug]: {}", self.buf);
        }
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
    pub(crate) fn new(config: ContractLogConfig) -> Self {
        let (sender, receiver) = mpsc::channel(16);
        let actor = RedirLogActor::new(receiver, config);
        tokio::spawn(run_redir_log_actor(actor));
        Self { sender }
    }

    pub(crate) fn start(&self, tx: &RawL2Transaction) {
        self.send_msg(RedirLogMsg::Session(hex::encode(tx.as_reader().hash())));
    }

    pub(crate) fn flush(&self, exit_code: i8) {
        self.send_msg(RedirLogMsg::Flush(exit_code));
    }

    pub(crate) fn append_log(&self, log: String) {
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
        };
    }
}
