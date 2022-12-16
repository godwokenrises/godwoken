---
name: Bug report
about: Create a report to help us improve
title: "[BUG] "
labels: bug
assignees: ''

---

## **Version**
- Godwoken v1 or v0?
- (Optional) Get the versions from `poly_version` RPC
  <details>
  <summary>command example</summary>

  ```sh
  curl https://godwoken-testnet-v1.ckbapp.dev -X POST \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc": "2.0", "method":"poly_version", "params": [], "id": 1}'
  ```
  </details>

## **Describe the bug**
A clear and concise description of what the bug is.

**To Reproduce**

Steps to reproduce the behavior:
1. Go to '...'
2. Some actions on '....'
3. See error

**Expected behavior**

A clear and concise description of what you expected to happen.

**Screenshots or Logs**

If applicable, add screenshots or logs to help explain your problem.

```log
some logs...
```

## **Additional context**
Add any other context about the problem here.
