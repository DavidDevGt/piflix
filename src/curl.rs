//! curl sin shell: la configuración (URL, cabeceras) viaja por stdin, así no aparece en `ps`.

use crate::prelude::*;

/// curl con stdin/stdout en tubería; el proceso muere si se descarta el futuro que lo espera.
pub(crate) fn spawn(args: &[&str]) -> std::io::Result<tokio::process::Child> {
    Command::new("curl")
        .args(args)
        .args(["-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
}

/// Ejecuta curl hasta el final y devuelve su salida si terminó bien.
pub(crate) async fn run(args: &[&str], config: &str) -> Option<Vec<u8>> {
    let mut child = spawn(args).ok()?;
    let mut stdin = child.stdin.take()?;
    stdin.write_all(config.as_bytes()).await.ok()?;
    drop(stdin);
    let out = child.wait_with_output().await.ok()?;
    out.status.success().then_some(out.stdout)
}
