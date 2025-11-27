# ErrorTK System Plan

## Architecture
A decoupled Client-Server architecture to separate the fragile scraping logic from the user interface.

### 1. Backend (Python)
*   **Role**: Data Provider & Scraper.
*   **Tech**: `FastAPI`, `Requests`, `python-dotenv`.
*   **Persistence**: `data/errors.json` (Single source of truth).
*   **Endpoints**:
    *   `POST /sync`: Triggers the scraper to fetch latest error IDs from upstream (Types 3, 4, 5), fetches question details, and merges with local DB.
    *   `GET /questions`: Returns the list of questions (supports filtering by source, status).
    *   `POST /questions/{id}/status`: Updates local status (e.g., 'mastered').

### 2. CLI (Rust)
*   **Role**: User Interface.
*   **Tech**: `Reqwest` (HTTP), `Serde` (JSON), `Clap` (Args), `Inquire` (Prompts).
*   **Features**:
    *   **Sync**: Commands the backend to refresh data.
    *   **Practice**: Interactive session to answer questions.
    *   **Review**: View wrong questions.

## Data Sources (per PRD)
1.  **Simulation (Type 5)**:
    *   Endpoint: `/api/v2/tk/getError?type=5&bookId=`
    *   Process: Get Papers -> Get Question IDs.
2.  **Real Exam (Type 4)**:
    *   Endpoint: `/api/v2/tk/getError?type=4&bookId=`
    *   Process: Get Papers -> Get Question IDs.
3.  **Famous (Type 3)**:
    *   Endpoint: `/api/v2/tk/getError?type=3&bookId=` (Gets Classes)
    *   Endpoint: `/api/v1/tk/getFamousByError?classId=...` (Gets Question IDs)

## Data Model (`errors.json`)

The storage will be structured as a JSON object with top-level keys representing the three error types, plus a metadata section.

```json
{
  "meta": {
    "last_sync": "2023-11-26T10:00:00Z",
    "version": "1.0"
  },
  "simulation": [
    {
      "id": 147851,
      "origin_name": "26曲艺3+1",
      "sub_name": "卷一",
      "type": 1, 
      "content": "Question text...",
      "options": [{"label": "A", "content": "..."}],
      "answer": ["A", "C"],
      "analysis": "Explanation text...", 
      "user_status": "new", 
      "last_reviewed": null,
      "comments": ["Comment 1", "Comment 2"]
    }
  ],
  "real": [
    {
      "id": 45327,
      "origin_name": "2010年考研真题",
      "sub_name": "", 
      "content": "...",
      "user_status": "new"
    }
  ],
  "famous": [
    {
      "id": 139904,
      "origin_name": "26大李子米鹏720",
      "sub_name": "第二章 世界的物质性及发展规律",
      "content": "...",
      "user_status": "mastered"
    }
  ]
}
```

### Field Definitions
*   `id`: The unique question ID from the source API.
*   `origin_name`: The high-level source (e.g., "26曲艺3+1").
*   `sub_name`: The specific section (e.g., "卷一" or "Chapter Name").
*   `user_status`: Local state tracking. Enum: `["new", "reviewing", "mastered"]`.
*   `analysis`: The explanation provided by the API (if available).

## TUI 工具（Rust）

目标：本地离线复习界面，读取 `backend/data/errors.json`，支持来源筛选、详情查看、状态标注与保存。

实现概览：
- 项目：`errorTK/tui`
- 依赖：`ratatui`、`crossterm`、`serde`、`serde_json`、`chrono`、`clap`
- 默认来源筛选：simulation + real
- 键位：
  - 导航：`j/k` 或 `↑/↓`
  - 切换来源：`1/2/3`（simulation/real/famous）
  - 切换显示：`a` 显示/隐藏答案与解析；`c` 显示/隐藏评论
  - 标注状态：`n/r/m`（new/reviewing/mastered），立即写回 JSON
  - 刷新文件：`Shift+R`；退出：`q`

运行：
```
cd errorTK/tui
cargo run --release -- --file ../backend/data/errors.json --source simulation --source real --show-comments
```
