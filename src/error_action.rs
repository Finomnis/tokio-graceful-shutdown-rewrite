#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorAction {
    Forward,
    CatchAndLocalShutdown,
}
