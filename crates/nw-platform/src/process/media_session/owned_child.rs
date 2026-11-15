//! Native lifetime owner for the new session route; no wrapper wait caches.
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(unix)]
pub(super) use unix::OwnedChild;
#[cfg(windows)]
pub(super) use windows::OwnedChild;

#[cfg(windows)]
pub(super) struct OutputReader {
    receiver: tokio::sync::mpsc::Receiver<Vec<u8>>,
    current: Vec<u8>,
    offset: usize,
}
#[cfg(windows)]
impl OutputReader {
    fn new(receiver: tokio::sync::mpsc::Receiver<Vec<u8>>) -> Self {
        Self { receiver, current: Vec::new(), offset: 0 }
    }
}
#[cfg(windows)]
impl tokio::io::AsyncRead for OutputReader {
    fn poll_read(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buffer: &mut tokio::io::ReadBuf<'_>) -> std::task::Poll<std::io::Result<()>> {
        if buffer.remaining() == 0 { return std::task::Poll::Ready(Ok(())); }
        if self.offset == self.current.len() {
            match self.receiver.poll_recv(cx) {
                std::task::Poll::Pending => return std::task::Poll::Pending,
                std::task::Poll::Ready(None) => return std::task::Poll::Ready(Ok(())),
                std::task::Poll::Ready(Some(bytes)) => { self.current = bytes; self.offset = 0; },
            }
        }
        let count = buffer.remaining().min(self.current.len() - self.offset);
        buffer.put_slice(&self.current[self.offset..self.offset + count]);
        self.offset += count;
        std::task::Poll::Ready(Ok(()))
    }
}
