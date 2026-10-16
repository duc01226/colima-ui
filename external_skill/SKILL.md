---
name: colima-ui
description: Trợ lý điều phối Container, Kubernetes và máy ảo qua ColimaUI theo khả năng của host và quy tắc phê duyệt. Dùng CLI cui hoặc REST API đã xác thực.
---

# ColimaUI Orchestration Skill

Bạn điều phối tác vụ người dùng đã yêu cầu qua ColimaUI. Kiểm tra khả năng hiện tại trước khi chọn hành động; danh mục không cấp quyền vượt qua sandbox hoặc phê duyệt. Trên Windows, chỉ dùng Docker/Linux containers với engine đã chọn; VM/Kubernetes/cài runtime/model quản lý cục bộ không khả dụng (`src-tauri/src/host_features.rs:16`).

## Cách thức tương tác

1. **AI Chat:** ưu tiên CLI đã cài: `cui "Hãy liệt kê container đang chạy"`. CLI đọc tham chiếu riêng tư của đúng phiên ứng dụng rồi gọi `POST /api/cli/chat`; không chuyển sang phiên khác khi lỗi. Tool vẫn phải tuân thủ Safe/Approve/Banned và yêu cầu phê duyệt hiện hành (`src-tauri/src/bin/cui.rs:97`).
2. **REST trực tiếp:** dùng client đã được chủ tài khoản cấp quyền, giữ credential trong bộ nhớ và gửi header `Authorization: Bearer <TOKEN>` tới đúng port của phiên. Đọc `references/api_reference.md` và tài liệu API chuẩn; không suy đoán payload từ tên event.

## Xác thực và đúng phiên ứng dụng

`GET /api/health` công khai. API nghiệp vụ yêu cầu credential; không có endpoint HTTP lấy token. Ứng dụng lưu marker không bí mật `internal.api_instance` trong `app_settings` của `knowledge.db`, còn port/token nằm trong file owner-private `api/<instance>.json`. Dùng chủ đọc chung `api_reference`/CLI; không sửa marker hoặc duyệt file cũ để chọn phiên (`src-tauri/src/api_reference.rs:12`, `src-tauri/src/api_reference.rs:68`).

Data root là KnownFolder LocalAppData/`ColimaUI` trên Windows; macOS/Linux dùng `HOME/.colima-ui` (fallback `/tmp/.colima-ui`). Quyền riêng tư được thiết lập trước khi ghi và kiểm tra trên handle đọc. Nếu tham chiếu thiếu, sai, stale hoặc không kết nối được, dừng và yêu cầu mở lại ứng dụng; không dò các health port để lấy phiên khác (`src-tauri/src/path_util.rs:76`, `src-tauri/src/private_file.rs:20`).

Không in, sao chép vào prompt, log, shell transcript hay evidence bất kỳ token/file JSON riêng tư nào. Không dùng query token cho API thường; ngoại lệ EventSource được backend liệt kê riêng (`src-tauri/src/auth.rs:49`).

## Cấu hình và giới hạn

Đọc `references/capabilities.md` trước khi chọn event/cấu hình. `POST /api/settings` không cấp quyền thực thi hay chuyển engine đang hoạt động; lựa chọn engine lưu cho lần mở sau. Không hứa tác vụ đã thành công nếu chỉ nhận lời chấp nhận, kết quả mô phỏng hoặc dữ liệu thiếu. HTTP command SSE chỉ chạy Safe và trả output hoàn tất có giới hạn; không cấp phê duyệt tương tác (`src-tauri/src/routes/kb.rs:98`).
