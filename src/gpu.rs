//! Capture every wgpu error class and pop scopes in LIFO order on every exit path.
pub(crate) struct ErrorScopes(Vec<wgpu::ErrorScopeGuard>);

impl ErrorScopes {
    pub fn new(device: &wgpu::Device) -> Self {
        Self(
            [
                wgpu::ErrorFilter::OutOfMemory,
                wgpu::ErrorFilter::Internal,
                wgpu::ErrorFilter::Validation,
            ]
            .into_iter()
            .map(|filter| device.push_error_scope(filter))
            .collect(),
        )
    }

    pub fn finish(mut self) -> Option<wgpu::Error> {
        let mut first = None;
        while let Some(scope) = self.0.pop() {
            let error = pollster::block_on(scope.pop());
            if first.is_none() {
                first = error;
            }
        }
        first
    }
}

impl Drop for ErrorScopes {
    fn drop(&mut self) {
        while let Some(scope) = self.0.pop() {
            drop(scope);
        }
    }
}
