#!/bin/bash
set -euo pipefail

# 使用说明
usage() {
  cat <<EOF
Usage: $(basename "$0") [options]

Options:
  -c, --include-comments     抓取评论（默认关闭）
      --no-comments          不抓取评论（默认值）
  -b, --batch-size N         题目详情抓取批大小，默认 50
  -t, --token TOKEN          直接传入 TOKEN（优先级最高）
  -s, --sources LIST         限定来源，逗号分隔：simulation,real,famous（默认全选）
  -i, --incremental          启用增量同步：仅抓取本地没有的新题
  -e, --env FILE             指定 .env 路径（默认 ../../.env）
      --sync-only            仅执行一次同步后退出（非交互）
  -h, --help                 显示帮助

环境变量（可选）：
  INCLUDE_COMMENTS=0|1       同 --include-comments / --no-comments
  BATCH_SIZE=N               同 --batch-size
  TOKEN=xxxx                 同 --token
  SOURCES=simulation,real    同 --sources
  INCREMENTAL=0|1            同 --incremental
EOF
}

# 进入 backend 目录
cd errorTK/backend || { echo "Failed to change directory to errorTK/backend"; exit 1; }

# 默认参数
INCLUDE_COMMENTS_DEFAULT="0"
BATCH_SIZE_DEFAULT="50"
ENV_FILE_DEFAULT="../../.env"

INCLUDE_COMMENTS="${INCLUDE_COMMENTS:-$INCLUDE_COMMENTS_DEFAULT}"
BATCH_SIZE="${BATCH_SIZE:-$BATCH_SIZE_DEFAULT}"
ENV_FILE="$ENV_FILE_DEFAULT"
SOURCES="${SOURCES-}"
INCREMENTAL="${INCREMENTAL-}"
SYNC_ONLY="0"

# 解析命令行参数
while [[ ${1-} ]]; do
  case "$1" in
    -c|--include-comments)
      INCLUDE_COMMENTS="1"; shift ;;
    --no-comments)
      INCLUDE_COMMENTS="0"; shift ;;
    -b|--batch-size)
      BATCH_SIZE="${2?请输入批大小数字}"; shift 2 ;;
    -t|--token)
      TOKEN="${2?请输入 TOKEN}"; shift 2 ;;
    -e|--env)
      ENV_FILE="${2?请输入 .env 路径}"; shift 2 ;;
    -s|--sources)
      SOURCES="${2?请输入来源列表，如 simulation,real}"; shift 2 ;;
    -i|--incremental)
      INCREMENTAL="1"; shift ;;
    --sync-only)
      SYNC_ONLY="1"; shift ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "未知参数: $1" >&2; echo; usage; exit 2 ;;
  esac
done

# 激活虚拟环境
source venv/bin/activate || { echo "Failed to activate virtual environment"; exit 1; }

# 若未显式传入 TOKEN，则尝试从 env 文件载入
if [ -z "${TOKEN-}" ] && [ -f "$ENV_FILE" ]; then
  echo "Loading TOKEN from $ENV_FILE"
  # 兼容 TOKEN="xxx" 或 TOKEN='xxx' 的写法，并去除首尾空白
  TOKEN=$(grep -E '^TOKEN=' "$ENV_FILE" | sed -E "s/^TOKEN=\s*//" | sed -E "s/^['\"]?(.*)['\"]?$/\1/" | tr -d '\r' | sed -E 's/\s+$//' || true)
fi

if [ -z "${TOKEN-}" ]; then
  echo "Warning: TOKEN 未设置。接口可能返回空数据或失败。可使用 --token 或 --env 指定。"
fi

export TOKEN INCLUDE_COMMENTS BATCH_SIZE SOURCES INCREMENTAL
echo "Config -> INCLUDE_COMMENTS=$INCLUDE_COMMENTS, BATCH_SIZE=$BATCH_SIZE, SOURCES=${SOURCES:-ALL}, INCREMENTAL=${INCREMENTAL:-0}"

if [ "$SYNC_ONLY" = "1" ]; then
  python - <<'PY'
from interactive_cli import sync_data
ok = sync_data()
print('sync-only finished:', ok)
PY
else
  # 运行交互式 CLI
  python interactive_cli.py
fi

# 退出虚拟环境
deactivate
