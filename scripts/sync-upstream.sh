#!/usr/bin/env bash
#
# sync-upstream.sh
#
# 1. 拉取上游 https://github.com/storytold/cadcraft 的 main 分支
# 2. 对照上游 ROADMAP.md 检测 M11（3D 建模里程碑）是否已经启动/完成，
#    并扫描代码中是否已出现 3D 实体命令（box/extrude/revolve/...）
# 3. 把上游 main 的新变化合并到本地 dev 分支
#
# 冲突策略：git merge -X theirs
#   - dev 上与上游不冲突的新增内容：保留
#   - 任何内容冲突：一律取上游 main 的版本
#   - 合并前自动创建备份分支，失败自动 abort，可一键回滚
#
# 用法：
#   scripts/sync-upstream.sh                 # 检测 + 合并（默认）
#   scripts/sync-upstream.sh --check-only    # 只检测汇报，不改动任何东西
#   scripts/sync-upstream.sh --remote upstream --dev dev
#
# 可用环境变量覆盖默认值：REMOTE_NAME、DEV_BRANCH
#
set -euo pipefail

UPSTREAM_URL="https://github.com/storytold/cadcraft.git"
REMOTE="${REMOTE_NAME:-upstream}"
MAIN_BRANCH="main"
DEV_BRANCH="${DEV_BRANCH:-dev}"
CHECK_ONLY=0

# ---------- 输出辅助 ----------
if [ -t 1 ]; then
  C_RED=$'\033[31m'; C_GRN=$'\033[32m'; C_YLW=$'\033[33m'
  C_CYN=$'\033[36m'; C_BLD=$'\033[1m';  C_RST=$'\033[0m'
else
  C_RED=""; C_GRN=""; C_YLW=""; C_CYN=""; C_BLD=""; C_RST=""
fi
info()  { printf '%s\n' "${C_CYN}==>${C_RST} $*"; }
ok()    { printf '%s\n' "${C_GRN}✔${C_RST} $*"; }
warn()  { printf '%s\n' "${C_YLW}!${C_RST} $*"; }
die()   { printf '%s\n' "${C_RED}✘ 错误:${C_RST} $*" >&2; exit 1; }

usage() {
  sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --check-only) CHECK_ONLY=1; shift ;;
    --remote)     REMOTE="${2:?}"; shift 2 ;;
    --dev)        DEV_BRANCH="${2:?}"; shift 2 ;;
    -h|--help)    usage 0 ;;
    *)            usage 1 ;;
  esac
done

# ---------- 前置检查 ----------
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "当前目录不在 git 仓库内"
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

# 配置 / 添加上游 remote
if git remote get-url "$REMOTE" >/dev/null 2>&1; then
  REMOTE_URL="$(git remote get-url "$REMOTE")"
  case "$REMOTE_URL" in
    *storytold/cadcraft*) : ;;
    *) warn "remote '$REMOTE' 当前指向 $REMOTE_URL，并非 storytold/cadcraft，继续按该地址拉取" ;;
  esac
else
  info "添加上游 remote：$REMOTE -> $UPSTREAM_URL"
  git remote add "$REMOTE" "$UPSTREAM_URL"
fi
UPSTREAM_REF="$REMOTE/$MAIN_BRANCH"

info "拉取上游最新代码（git fetch $REMOTE $MAIN_BRANCH）"
git fetch --prune "$REMOTE" "$MAIN_BRANCH"
git rev-parse --verify "$UPSTREAM_REF" >/dev/null 2>&1 || die "拉取后仍找不到 $UPSTREAM_REF"

# ---------- dev 分支与工作区状态 ----------
if git show-ref --verify --quiet "refs/heads/$DEV_BRANCH"; then
  ORIG_BRANCH="$(git symbolic-ref --quiet --short HEAD || true)"
  if [ "$ORIG_BRANCH" != "$DEV_BRANCH" ]; then
    info "切换到 $DEV_BRANCH 分支"
    git checkout "$DEV_BRANCH"
  fi
else
  info "本地不存在 $DEV_BRANCH，基于 $UPSTREAM_REF 创建"
  git checkout -b "$DEV_BRANCH" "$UPSTREAM_REF"
fi

# tracked 文件必须干净（untracked 文件仅警告）
git diff-files --quiet || die "工作区有未暂存的改动，请先提交或 stash 后再运行"
git diff-index --cached --quiet HEAD || die "暂存区有未提交的改动，请先提交或 stash 后再运行"
if [ -n "$(git ls-files --others --exclude-standard)" ]; then
  warn "存在未跟踪文件；若与上游新增文件重名将导致合并失败（届时会自动 abort）"
fi

# ---------- 变化检测 ----------
echo
printf '%s\n' "${C_BLD}──────── 上游 main 相对本地 dev 的变化 ────────${C_RST}"
# left=ahead(dev 独有)  right=behind(上游新增)
read -r AHEAD BEHIND < <(git rev-list --left-right --count "$DEV_BRANCH...$UPSTREAM_REF")
echo "dev 独有提交：$AHEAD    上游 main 新增提交：$BEHIND"

if [ "$BEHIND" -gt 0 ]; then
  echo
  echo "上游新增提交（最多显示 30 条）："
  git log --oneline --no-merges "$DEV_BRANCH..$UPSTREAM_REF" | head -n 30 | sed 's/^/  /'
  echo
  echo "变更概览（文件级统计，最多 40 行）："
  git diff --stat "$DEV_BRANCH...$UPSTREAM_REF" | head -n 40 | sed 's/^/  /'
else
  ok "上游 main 没有新提交，dev 已是最新"
fi

# ---------- M11 检测 ----------
echo
printf '%s\n' "${C_BLD}──────── M11（3D 建模）里程碑检测 ────────${C_RST}"

m11_row_of() { # $1 = ref
  git show "$1:ROADMAP.md" 2>/dev/null \
    | grep -E '^[[:space:]]*\|[[:space:]]*M11[[:space:]]*\|' || true
}
status_cell() { # 从 ROADMAP 表格行提取第 4 列（Status）
  awk -F'|' '{gsub(/^[[:space:]]+|[[:space:]]+$/, "", $4); print $4}'
}

LOCAL_ROW="$(m11_row_of "$DEV_BRANCH")"
UP_ROW="$(m11_row_of "$UPSTREAM_REF")"

if [ -z "$UP_ROW" ]; then
  warn "上游 ROADMAP.md 中未找到 M11 行（表格结构可能已调整），请人工查看 $UPSTREAM_REF:ROADMAP.md"
else
  UP_STATUS="$(printf '%s' "$UP_ROW" | status_cell)"
  printf '上游 M11 状态：%s\n' "$UP_STATUS"
  case "$UP_STATUS" in
    *done*|*100%*)
      printf '%s\n' "${C_GRN}✔ M11 在 ROADMAP 中已标记完成${C_RST}" ;;
    0%|"")
      printf '%s\n' "${C_YLW}● M11 尚未开始（状态仍为 0%）${C_RST}" ;;
    *)
      printf '%s\n' "${C_GRN}▶ M11 看起来已经启动${C_RST}（上面是上游标注的状态）" ;;
  esac

  if [ -n "$LOCAL_ROW" ]; then
    LOCAL_STATUS="$(printf '%s' "$LOCAL_ROW" | status_cell)"
    if [ "$LOCAL_STATUS" = "$UP_STATUS" ]; then
      echo "本地 dev 与上游的 M11 状态一致，无变化"
    else
      printf '%s\n' "${C_CYN}M11 状态发生变化：${C_RST}[$LOCAL_STATUS] -> [$UP_STATUS]"
    fi
  fi
fi

# 代码信号：上游是否已经注册 3D 实体/布尔/UCS 命令
echo
echo "扫描上游代码中的 M11 命令实现信号……"
SIGNAL_RE='CommandSpec::new\("(box|wedge|cone|sphere|cylinder|torus|pyramid|polysolid|extrude|revolve|loft|sweep|presspull|union|subtract|intersect|3dorbit|ucs)"'
SIGNALS="$(git grep -I -n -E "$SIGNAL_RE" "$UPSTREAM_REF" -- crates 2>/dev/null || true)"
if [ -n "$SIGNALS" ]; then
  printf '%s\n' "${C_GRN}✔ 上游已出现 3D 相关命令注册：${C_RST}"
  printf '%s\n' "$SIGNALS" | head -n 30 | sed 's/^/  /'
else
  printf '%s\n' "${C_YLW}● 上游 crates/ 中未发现 box/extrude/revolve/3dorbit/ucs 等命令注册，M11 尚无代码落地${C_RST}"
fi

# ---------- 合并 ----------
echo
if [ "$CHECK_ONLY" -eq 1 ]; then
  info "--check-only 模式：不执行合并。检测结束。"
  exit 0
fi

if [ "$BEHIND" -eq 0 ]; then
  ok "没有需要合并的内容，结束。"
  exit 0
fi

STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP="backup/$DEV_BRANCH-pre-m11sync-$STAMP"
info "合并前创建备份分支：$BACKUP"
git branch "$BACKUP" "$DEV_BRANCH"

info "执行合并：git merge -X theirs $UPSTREAM_REF（冲突一律取上游 main）"
if ! git merge -X theirs --no-edit -m "Merge upstream $MAIN_BRANCH into $DEV_BRANCH (conflicts favor upstream)" "$UPSTREAM_REF"; then
  warn "合并未能自动完成（存在 -X theirs 也处理不了的冲突，如删除/改名冲突）"
  git merge --abort 2>/dev/null || true
  die "已 abort 合并，dev 保持原样，未做任何改动。可检查后手动合并；备份分支：$BACKUP"
fi

echo
ok "合并完成。dev 上不冲突的新增内容已保留，冲突部分均采用上游 main 的版本。"
echo
echo "后续操作："
echo "  查看合并结果： git log --oneline -n 5"
echo "  如需回滚：     git reset --hard $BACKUP"
echo "  确认无误推送： git push -u origin $DEV_BRANCH   （脚本不会自动推送）"
