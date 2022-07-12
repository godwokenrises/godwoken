use gw_config::ContractLogConfig;
use gw_types::{
    bytes::{BufMut, BytesMut},
    packed::RawL2Transaction,
};
use tokio::sync::mpsc;

#[derive(Debug)]
pub(crate) enum RedirLogMsg {
    Session(RawL2Transaction),
    Log(Vec<u8>),
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
            RedirLogMsg::Session(tx) => self.ctx.setup(tx),
            RedirLogMsg::Log(log) => self.ctx.append_log(&log),
            RedirLogMsg::Flush(exit_code) => self.ctx.flush(exit_code),
        }
    }
}

// We can store the whole context of contract execution with tx, logs and exit code.
struct Context {
    tx: Option<RawL2Transaction>,
    sink: BytesMut,
    config: ContractLogConfig,
}

impl Context {
    fn init(config: ContractLogConfig) -> Self {
        Self {
            tx: None,
            sink: BytesMut::with_capacity(1024),
            config,
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
            if self.config == ContractLogConfig::Redirect
                || (self.config == ContractLogConfig::RedirectError && exit_code != 0)
            {
                log::debug!("[contract debug]: {}", s);
                log::debug!("contract exit code: {}", exit_code);
            }
            //send to senty if exit code != 0
            //format:
            //  tx_hash
            //  [contrace logs]
            //  ...
            //  exit code: 3
            if exit_code != 0 {
                let mut entries: Vec<String> = Vec::with_capacity(3);
                if let Some(tx) = &self.tx {
                    let tx_hash = hex::encode(tx.as_reader().hash());
                    entries.push(tx_hash);
                }
                entries.push(s.to_string());
                entries.push(format!("exit code: {}", exit_code));
                let msg = entries.join("\n");
                sentry::capture_message(&msg, sentry::Level::Error);
            }
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
    sender: Option<mpsc::Sender<RedirLogMsg>>,
}

impl RedirLogHandler {
    pub(crate) fn new(config: ContractLogConfig) -> Self {
        //Don't spawn tokio task in default mode.
        let sender = if config != ContractLogConfig::Default {
            let (sender, receiver) = mpsc::channel(16);
            let actor = RedirLogActor::new(receiver, config);
            tokio::spawn(run_redir_log_actor(actor));
            Some(sender)
        } else {
            None
        };
        Self { sender }
    }

    pub(crate) fn start(&self, tx: RawL2Transaction) {
        self.send_msg(RedirLogMsg::Session(tx));
    }

    pub(crate) fn flush(&self, exit_code: i8) {
        self.send_msg(RedirLogMsg::Flush(exit_code));
    }

    pub(crate) fn append_log(&self, log: Vec<u8>) {
        self.send_msg(RedirLogMsg::Log(log));
    }

    fn send_msg(&self, msg: RedirLogMsg) {
        match &self.sender {
            Some(sender) => match sender.try_send(msg) {
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
            },
            None => {
                if let RedirLogMsg::Log(log) = msg {
                    if let Ok(s) = std::str::from_utf8(&log) {
                        log::debug!("[contract debug]: {}", s);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use gw_types::{packed::RawL2Transaction, prelude::*};

    use super::{Context, RedirLogHandler};

    #[test]
    fn redir_sentry_test() {
        let mut ctx = Context::init(gw_config::ContractLogConfig::Redirect);
        let tx = RawL2Transaction::new_builder()
            .chain_id(0.pack())
            .from_id(1u32.pack())
            .to_id(2u32.pack())
            .nonce(0u32.pack())
            .build();

        let event = sentry::test::with_captured_events(|| {
            ctx.setup(tx.clone());
            ctx.append_log(b"debug log");
            ctx.flush(1);
        });
        let target = Some("05bb2c2e17393dea8bd1206a0b2ab104dec2593f1b91be4d764d3904b3a56847\ndebug log\n\nexit code: 1".to_string());
        assert_eq!(target, event[0].message);
    }

    #[test]
    #[should_panic(
        expected = "there is no reactor running, must be called from the context of a Tokio 1.x runtime"
    )]
    fn redir_panic_test() {
        let _handler = RedirLogHandler::new(gw_config::ContractLogConfig::Redirect);
    }

    #[test]
    #[should_panic(
        expected = "there is no reactor running, must be called from the context of a Tokio 1.x runtime"
    )]
    fn redir_err_panic_test() {
        let _handler = RedirLogHandler::new(gw_config::ContractLogConfig::RedirectError);
    }
}
