use reqwest::Client;
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use std::env;
use std::io::{self, Write};

#[derive(Clone)]
struct I18n {
    lang: String,
}

impl I18n {
    fn new() -> Self {
        let mut lang = env::var("LANG").unwrap_or_else(|_| "en".to_string());

        // Try reading from SQLite knowledge.db
        let db_path = colima_ui_lib::path_util::try_app_data_dir()
            .ok()
            .map(|p| p.join("knowledge.db"));

        if let Some(db_path) = db_path {
            if let Ok(conn) =
                Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            {
                if let Ok(Some(value)) = colima_ui_lib::app_settings::read(&conn, "app.language") {
                    lang = value;
                }
            }
        }

        // Normalize language
        if lang.starts_with("vi") {
            lang = "vi".to_string();
        } else if lang.starts_with("zh") {
            lang = "zh".to_string();
        } else {
            lang = "en".to_string();
        }

        Self { lang }
    }

    fn t<'a>(&self, key: &'a str) -> &'a str {
        match (self.lang.as_str(), key) {
            ("vi", "usage") => "Sử dụng: cui <lệnh...>",
            ("vi", "example") => "Ví dụ: cui Hãy liệt kê danh sách các container đang chạy",
            ("vi", "err_token") => "Lỗi: Không thể đọc thông tin truy cập riêng tư của ứng dụng.",
            ("vi", "err_running") => "Vui lòng đảm bảo Colima UI đang chạy ở chế độ nền.",
            ("vi", "thinking") => "Đang suy nghĩ... ",
            ("vi", "err_parse") => "Thành công, nhưng không thể phân tích kết quả:",
            ("vi", "err_api") => "Lỗi API",
            ("vi", "err_conn") => "Lỗi kết nối",
            ("vi", "err_port") => {
                "Không thể kết nối đến phiên ColimaUI đã chọn. Hãy khởi chạy lại ứng dụng."
            }

            ("zh", "usage") => "用法: cui <提示词...>",
            ("zh", "example") => "示例: cui 请列出正在运行的容器",
            ("zh", "err_token") => "错误: 无法读取应用的私有访问凭据。",
            ("zh", "err_running") => "请确保 Colima UI 正在后台运行。",
            ("zh", "thinking") => "思考中... ",
            ("zh", "err_parse") => "成功，但无法解析结果:",
            ("zh", "err_api") => "API 错误",
            ("zh", "err_conn") => "连接错误",
            ("zh", "err_port") => "无法连接到所选的 ColimaUI 实例。请重新启动应用。",

            // Default fallback (English)
            (_, "usage") => "Usage: cui <prompt...>",
            (_, "example") => "Example: cui Please list running containers",
            (_, "err_token") => "Error: Could not read the app’s private access reference.",
            (_, "err_running") => "Please ensure Colima UI is running in the background.",
            (_, "thinking") => "Thinking... ",
            (_, "err_parse") => "Success, but could not parse result:",
            (_, "err_api") => "API Error",
            (_, "err_conn") => "Connection Error",
            (_, "err_port") => {
                "The selected ColimaUI instance is unavailable. Relaunch the app and retry."
            }

            (_, _) => key,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let i18n = I18n::new();

    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        println!("{}", i18n.t("usage"));
        println!("{}", i18n.t("example"));
        return Ok(());
    }

    let prompt = args.join(" ");
    let reference = match colima_ui_lib::api_reference::read_latest() {
        Ok(reference) => reference,
        Err(_) => {
            eprintln!("{}", i18n.t("err_token"));
            eprintln!("{}", i18n.t("err_running"));
            std::process::exit(1);
        }
    };
    let port = reference.port();
    print!("{}", i18n.t("thinking"));
    io::stdout().flush()?;
    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/api/cli/chat", port);
    let body = json!({
        "prompt": prompt
    });

    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {}", reference.token()))
        .json(&body)
        .send()
        .await;

    // Clear the "Thinking... " text
    print!("\r\x1b[2K");
    io::stdout().flush()?;

    match response {
        Ok(res) => {
            let status = res.status();
            let text = res.text().await?;
            if status.is_success() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(result) = json.get("result").and_then(|v| v.as_str()) {
                        println!("{}", result);
                    } else {
                        println!("{} {}", i18n.t("err_parse"), text);
                    }
                } else {
                    println!("{}", text);
                }
            } else {
                eprintln!("{} ({}): {}", i18n.t("err_api"), status, text);
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("{}: {}", i18n.t("err_conn"), e);
            eprintln!("{}", i18n.t("err_port"));
            std::process::exit(1);
        }
    }

    Ok(())
}
