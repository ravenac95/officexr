# Picks the Cloudflare Pages preview deployments that pages-cleanup.yml deletes.
#
# Input: an array of {id, branch, environment, created_on}, one per deployment.
# Args (--argjson):
#   $prod     the project's production branch (also "main" is always kept)
#   $repo     "owner/name"; PR head branches only count when the head is in this repo
#   $open     open PRs, [{number, head, repo}]
#   $closed   closed PRs to clean up, [{number, head, repo}]
#   $branches the repo's branch names, or null to skip the "branch is gone" rule
# Output: the input entries to delete.
#
# A deployment is deleted only when it is a preview, not on the production
# branch, and either:
#   - its branch is pr-<n> (pages-deploy.yml since #31) and PR n is closed, or
#   - its branch is a head branch from before #31, no open PR uses it, and a
#     closed PR used it or the branch no longer exists.
def heads($prs): [$prs[] | select(.repo == $repo) | .head];

($open | map(.number)) as $open_numbers
| ($closed | map(.number)) as $closed_numbers
| heads($open) as $open_heads
| heads($closed) as $closed_heads
| map(select(
    .environment == "preview"
    and (.branch | type) == "string" and .branch != ""
    and .branch != $prod and .branch != "main"
    and (.branch as $b
      | if ($b | test("^pr-[0-9]+$")) then
          ($b[3:] | tonumber) as $n
          | ($n | IN($closed_numbers[])) and ($n | IN($open_numbers[]) | not)
        else
          ($b | IN($open_heads[]) | not)
          and (($b | IN($closed_heads[]))
               or ($branches != null and ($b | IN($branches[]) | not)))
        end)
  ))
