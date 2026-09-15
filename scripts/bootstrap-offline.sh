#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
reference_dir=${1:?请指定已有依赖的 cc-switch 工程路径}
if [ -e "$project_dir/node_modules" ]; then
  echo 'node_modules 已存在，保留原目录。'
else
  if [ ! -d "$reference_dir/node_modules/.pnpm" ]; then
    echo '参考目录缺少已安装的 pnpm 依赖。' >&2
    exit 1
  fi
  cp -R "$reference_dir/node_modules" "$project_dir/node_modules"
fi
node "$project_dir/scripts/align-tauri-cache.mjs"
echo '离线依赖已复制到 Skill Manager 工程。'
