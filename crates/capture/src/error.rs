use thiserror::Error;

/// Errores del motor de capturas.
#[derive(Debug, Error)]
pub enum Error {
    #[error("error de GDI: {0}")]
    Gdi(String),

    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),

    #[error("no se pudo codificar el PNG: {0}")]
    Encode(String),

    #[error("dimensiones inválidas: {0}x{1}")]
    InvalidDimensions(u32, u32),

    #[error("buffer de {got} bytes, se esperaban {expected}")]
    BufferSize { expected: usize, got: usize },
}

pub type Result<T> = std::result::Result<T, Error>;
