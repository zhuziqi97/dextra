//! Cerebro MCP stdio 伴生进程入口。

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let mut socket = None;
    let mut token = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => {
                println!("dextra-cerebro-mcp-bridge --socket-path <path> --token <temporary-token>");
                return;
            }
            "--socket-path" => socket = args.next(),
            "--token" => token = args.next(),
            _ => {
                eprintln!("未知 Bridge 参数");
                std::process::exit(2);
            }
        }
    }
    let (Some(socket), Some(token)) = (socket, token) else {
        eprintln!("缺少 --socket-path 或 --token");
        std::process::exit(2);
    };
    if let Err(error) = dextra_lib::cerebro::mcp_bridge::run(socket, token).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
    // stdin 使用阻塞线程；父连接退出时不能等待 Agent 再关闭 stdin 才结束进程。
    std::process::exit(0);
}
