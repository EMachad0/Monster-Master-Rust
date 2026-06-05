repo_root := `git rev-parse --show-toplevel`
parent_dir := repo_root / ".."

[doc("Create a new worktree branched from origin/main")]
new branch:
    #!/usr/bin/env bash
    set -euo pipefail
    worktree_path="{{parent_dir}}/{{branch}}"

    echo "==> Fetching origin/main"
    git fetch origin main

    echo "==> Creating worktree at ../{{branch}}"
    git worktree add "${worktree_path}" -b "{{branch}}" --no-track origin/main

    cd "${worktree_path}"

    # echo "==> Generating Claude worktree context"
    # BRANCH="{{branch}}" OFFSET="${offset}" envsubst \
    #     < "{{repo_root}}/.just/templates/worktree.template.md" \
    #     > .claude/worktree.local.md

    echo "==> Worktree '{{branch}}' is ready at ../{{branch}}"

[doc("Create a new worktree and open it in a zellij tab")]
new_ui branch: (new branch) (zellij branch)

[doc("Remove a worktree and its resources")]
rm branch=`git branch --show-current` force="":
    #!/usr/bin/env bash
    set -euo pipefail
    force="{{force}}"
    original_dir="${PWD}"

    if [ "{{branch}}" = "main" ]; then
        echo "Error: refusing to remove 'main'."
        exit 1
    fi

    worktree_path="{{parent_dir}}/{{branch}}"
    is_tracked=$(git worktree list | grep -c "\[{{branch}}\]" || true)

    if [ "${is_tracked}" -eq 0 ] && [ ! -d "${worktree_path}" ]; then
        echo "Error: worktree for branch '{{branch}}' does not exist."
        exit 1
    fi

    echo "==> Removing worktree '{{branch}}'"

    if [ -d "${worktree_path}" ]; then
        just wk::_stop_dev "${worktree_path}"

        echo "==> Stopping docker containers"
        (cd "${worktree_path}" && docker compose down 2>/dev/null) || true
    fi

    cd "{{parent_dir}}/main"

    if [ "${is_tracked}" -gt 0 ]; then
        echo "==> Removing git worktree"
        git worktree remove "${worktree_path}" --force
    fi

    if [ -d "${worktree_path}" ]; then
        echo "==> Cleaning up leftover directory"
        rm -rf "${worktree_path}"
    fi

    echo "==> Deleting branch '{{branch}}'"
    if [ "${force}" = "force" ]; then
        git branch -D "{{branch}}"
    else
        git branch -d "{{branch}}"
    fi

    echo "==> Worktree '{{branch}}' removed"

    if [ -n "${ZELLIJ_SESSION_NAME:-}" ] && echo "${original_dir}" | grep -q "{{branch}}"; then
        zellij action close-tab
    fi

[doc("List all worktrees")]
list:
    @git worktree list

[doc("Open a worktree in a zellij tab")]
zellij branch:
    #!/usr/bin/env bash
    set -euo pipefail
    worktree_path="{{parent_dir}}/{{branch}}"

    if [ -z "${ZELLIJ_SESSION_NAME:-}" ]; then
        echo "Error: not inside a zellij session. Start zellij first."
        exit 1
    fi

    if [ ! -d "${worktree_path}" ]; then
        echo "Error: worktree '../{{branch}}' does not exist. Run 'just wk::new {{branch}}' first."
        exit 1
    fi

    zellij action new-tab \
        --layout "{{repo_root}}/.zellij/layout.kdl" \
        --cwd "${worktree_path}" \
        --name "{{branch}}"

_kill_port port timeout="3":
    #!/usr/bin/env bash
    set -euo pipefail
    pid=$(lsof -ti :{{port}} 2>/dev/null || true)

    if [ -z "${pid}" ]; then
        exit 0
    fi

    kill "${pid}" 2>/dev/null || exit 0

    max_ticks=$(( {{timeout}} * 10 ))
    for (( i=0; i<max_ticks; i++ )); do
        kill -0 "${pid}" 2>/dev/null || exit 0
        sleep 0.1
    done

    kill -9 "${pid}" 2>/dev/null || true
