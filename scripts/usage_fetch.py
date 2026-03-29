#!/usr/bin/env python3
"""Fetch Claude/OpenAI usage caches for NeoZeus.

This is a vendored helper modeled after Zeus's usage fetchers. It owns provider auth lookup,
network fetches, and cache-file writes so the Rust app can stay cache-driven and UI-focused.
"""

from __future__ import annotations

import json
import os
from glob import glob
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request


CLAUDE_USAGE_URL = "https://api.anthropic.com/api/oauth/usage"
CLAUDE_CREDENTIALS_FILE = Path.home() / ".claude" / ".credentials.json"
OPENAI_AUTH_FILE = Path.home() / ".pi" / "agent" / "auth.json"


def _resolve_state_dir() -> Path:
    explicit = os.environ.get("NEOZEUS_STATE_DIR", "").strip()
    if explicit:
        path = Path(os.path.expanduser(explicit))
    else:
        xdg_state_home = os.environ.get("XDG_STATE_HOME", "").strip()
        home = os.environ.get("HOME", "").strip()
        xdg_config_home = os.environ.get("XDG_CONFIG_HOME", "").strip()
        if xdg_state_home:
            path = Path(xdg_state_home) / "neozeus"
        elif home:
            path = Path(home) / ".local" / "state" / "neozeus"
        elif xdg_config_home:
            path = Path(xdg_config_home) / "neozeus"
        else:
            path = Path("/tmp") / "neozeus"
    path.mkdir(parents=True, exist_ok=True)
    return path


STATE_DIR = _resolve_state_dir()
CLAUDE_CACHE = Path(
    os.environ.get("NEOZEUS_CLAUDE_USAGE_CACHE", "").strip() or STATE_DIR / "claude-usage-cache.json"
)
OPENAI_CACHE = Path(
    os.environ.get("NEOZEUS_OPENAI_USAGE_CACHE", "").strip() or STATE_DIR / "openai-usage-cache.json"
)
CLAUDE_LOG = Path(
    os.environ.get("NEOZEUS_CLAUDE_USAGE_LOG", "").strip() or STATE_DIR / "claude-usage.log"
)
OPENAI_LOG = Path(
    os.environ.get("NEOZEUS_OPENAI_USAGE_LOG", "").strip() or STATE_DIR / "openai-usage.log"
)
CLAUDE_BACKOFF = Path(
    os.environ.get("NEOZEUS_CLAUDE_USAGE_BACKOFF", "").strip()
    or STATE_DIR / "claude-usage-backoff-until.txt"
)


def _atomic_write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", dir=path.parent, delete=False) as temp:
        temp.write(content)
        temp_path = Path(temp.name)
    temp_path.replace(path)


def _log(path: Path, message: str) -> None:
    try:
        timestamp = time.strftime("%Y-%m-%d %H:%M:%S")
        with path.open("a", encoding="utf-8") as handle:
            handle.write(f"[{timestamp}] {message}\n")
    except OSError:
        pass


def _clear_file_if_exists(path: Path) -> None:
    try:
        path.unlink(missing_ok=True)
    except OSError:
        pass


def _write_claude_backoff_until(until_epoch_s: int) -> None:
    try:
        _atomic_write_text(CLAUDE_BACKOFF, str(until_epoch_s))
    except OSError:
        pass


def _parse_retry_after_seconds(headers: object) -> int | None:
    try:
        raw = headers.get("Retry-After")
    except AttributeError:
        return None
    if raw is None:
        return None
    try:
        return max(0, int(str(raw).strip()))
    except ValueError:
        return None


# ---------------------------------------------------------------------------
# Claude
# ---------------------------------------------------------------------------


def _load_claude_oauth_info() -> tuple[str, bool]:
    try:
        data = json.loads(CLAUDE_CREDENTIALS_FILE.read_text())
        oauth = data.get("claudeAiOauth", {})
        token = oauth.get("accessToken") or ""
        expires_at = int(oauth.get("expiresAt") or 0)
        expired = expires_at > 0 and int(time.time() * 1000) > expires_at
        return token, expired
    except (OSError, json.JSONDecodeError, TypeError, ValueError):
        return "", False


def _refresh_claude_oauth_token() -> bool:
    try:
        proc = subprocess.run(
            ["claude", "-p", "hi", "--model", "haiku"],
            capture_output=True,
            text=True,
            timeout=20,
            start_new_session=True,
        )
    except (subprocess.TimeoutExpired, FileNotFoundError, OSError) as error:
        _log(CLAUDE_LOG, f"token refresh failed: {type(error).__name__}: {error}")
        return False

    if proc.returncode != 0:
        tail = (proc.stderr or "").strip().splitlines()[-1:] or [""]
        _log(
            CLAUDE_LOG,
            f"token refresh exited non-zero code={proc.returncode} tail={tail[0]!r}",
        )
        return False

    _log(CLAUDE_LOG, "token refresh completed")
    return True


def _fetch_claude_usage_once(access_token: str) -> tuple[int, str, int | None]:
    try:
        request = urllib.request.Request(
            CLAUDE_USAGE_URL,
            method="GET",
            headers={
                "Content-Type": "application/json",
                "Authorization": f"Bearer {access_token}",
                "anthropic-beta": "oauth-2025-04-20",
            },
        )
        with urllib.request.urlopen(request, timeout=5) as response:
            status = int(getattr(response, "status", 200) or 200)
            body = response.read().decode(errors="replace")
            return status, body, _parse_retry_after_seconds(response.headers)
    except urllib.error.HTTPError as error:
        body = error.read(500).decode(errors="replace")
        return int(error.code), body, _parse_retry_after_seconds(error.headers)
    except (urllib.error.URLError, TimeoutError, OSError, ValueError) as error:
        _log(CLAUDE_LOG, f"usage fetch failed: {type(error).__name__}: {error}")
        return 0, "", None


def fetch_claude_usage() -> int:
    token, expired = _load_claude_oauth_info()
    if not token or expired:
        _log(CLAUDE_LOG, "oauth token missing/expired; refreshing")
        if not _refresh_claude_oauth_token():
            return 1
        token, _ = _load_claude_oauth_info()
        if not token:
            _log(CLAUDE_LOG, "token still missing after refresh")
            return 1

    status, body, retry_after_seconds = _fetch_claude_usage_once(token)
    if status in (401, 403):
        _log(CLAUDE_LOG, f"usage API rejected token ({status}); refreshing once")
        if not _refresh_claude_oauth_token():
            return 1
        token, _ = _load_claude_oauth_info()
        if not token:
            _log(CLAUDE_LOG, "token missing after auth retry")
            return 1
        status, body, retry_after_seconds = _fetch_claude_usage_once(token)

    if status == 429:
        backoff_seconds = retry_after_seconds if retry_after_seconds is not None else 900
        backoff_until = int(time.time()) + max(60, backoff_seconds)
        _write_claude_backoff_until(backoff_until)
        _log(
            CLAUDE_LOG,
            f"usage API rate limited status=429 backoff_until={backoff_until} retry_after={retry_after_seconds}",
        )
        return 1

    if status != 200 or not body:
        _log(CLAUDE_LOG, f"usage API request failed status={status}")
        return 1

    try:
        parsed = json.loads(body)
    except (json.JSONDecodeError, TypeError, ValueError):
        _log(CLAUDE_LOG, "usage API returned non-JSON body")
        return 1

    if not isinstance(parsed, dict) or not parsed.get("five_hour"):
        _log(CLAUDE_LOG, "usage API payload missing five_hour")
        return 1

    try:
        _atomic_write_text(CLAUDE_CACHE, body)
        _clear_file_if_exists(CLAUDE_BACKOFF)
        _log(CLAUDE_LOG, f"cached Claude usage to {CLAUDE_CACHE}")
        return 0
    except OSError as error:
        _log(CLAUDE_LOG, f"failed writing Claude cache: {error}")
        return 1


# ---------------------------------------------------------------------------
# OpenAI
# ---------------------------------------------------------------------------


def _load_openai_access_token() -> str:
    try:
        data = json.loads(OPENAI_AUTH_FILE.read_text())
        token_info = data.get("openai-codex", {})
        token = token_info.get("access", "")
        expires = int(token_info.get("expires", 0) or 0)
        if token and (not expires or expires > int(time.time() * 1000)):
            _log(OPENAI_LOG, f"loaded oauth access token from {OPENAI_AUTH_FILE}")
            return token
        if token:
            _log(OPENAI_LOG, "oauth access token appears expired, trying anyway")
            return token
    except (OSError, json.JSONDecodeError, ValueError, TypeError) as error:
        _log(OPENAI_LOG, f"failed to read {OPENAI_AUTH_FILE}: {error}")
    return ""


def _scan_kitty_env_for_openai_key() -> str:
    try:
        for socket in glob("/tmp/kitty-*"):
            raw = subprocess.run(
                ["kitty", "@", "--to", f"unix:{socket}", "ls"],
                capture_output=True,
                text=True,
                timeout=2,
            ).stdout
            data = json.loads(raw) if raw else []
            for os_window in data:
                for tab in os_window.get("tabs", []):
                    for window in tab.get("windows", []):
                        key = window.get("env", {}).get("OPENAI_API_KEY")
                        if key:
                            _log(OPENAI_LOG, f"auth source: kitty env socket={socket}")
                            return key
    except (subprocess.TimeoutExpired, FileNotFoundError, OSError, json.JSONDecodeError) as error:
        _log(OPENAI_LOG, f"failed to scan kitty env for OPENAI_API_KEY: {error}")
    return ""


def _write_openai_cache(payload: dict[str, object]) -> int:
    try:
        _atomic_write_text(OPENAI_CACHE, json.dumps(payload))
        _log(OPENAI_LOG, f"cached OpenAI usage to {OPENAI_CACHE}")
        return 0
    except OSError as error:
        _log(OPENAI_LOG, f"failed writing OpenAI cache: {error}")
        return 1


def fetch_openai_usage() -> int:
    api_key = os.environ.get("OPENAI_API_KEY", "")
    if api_key:
        _log(OPENAI_LOG, "auth source: OPENAI_API_KEY env")
    if not api_key:
        api_key = _scan_kitty_env_for_openai_key()
    if not api_key:
        api_key = _load_openai_access_token()
        if api_key:
            _log(OPENAI_LOG, "auth source: pi oauth token")
    if not api_key:
        _log(OPENAI_LOG, "no auth found (OPENAI_API_KEY or pi oauth)")
        return 1

    base_urls = [
        "https://chatgpt.com/backend-api",
        "https://chat.openai.com/backend-api",
    ]
    for base in base_urls:
        url = f"{base}/wham/usage"
        _log(OPENAI_LOG, f"requesting {url}")
        request = urllib.request.Request(
            url,
            method="GET",
            headers={
                "Authorization": f"Bearer {api_key}",
                "User-Agent": "neozeus",
                "Content-Type": "application/json",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=5) as response:
                body = response.read().decode(errors="replace")
                data = json.loads(body)
                rate_limit = data.get("rate_limit") or {}
                primary = rate_limit.get("primary_window") or {}
                secondary = rate_limit.get("secondary_window") or {}

                def _pct(window: dict[str, object]) -> float:
                    try:
                        return float(window.get("used_percent", 0.0))
                    except (TypeError, ValueError):
                        return 0.0

                def _reset_at(window: dict[str, object]) -> str:
                    raw = window.get("reset_at")
                    if raw is None:
                        return ""
                    try:
                        seconds = int(raw)
                        from datetime import datetime, timezone

                        return datetime.fromtimestamp(seconds, tz=timezone.utc).isoformat()
                    except (TypeError, ValueError, OSError):
                        return str(raw)

                payload = {
                    "requests_limit": int(primary.get("limit", 0) or 0),
                    "requests_remaining": int(primary.get("remaining", 0) or 0),
                    "tokens_limit": int(secondary.get("limit", 0) or 0),
                    "tokens_remaining": int(secondary.get("remaining", 0) or 0),
                    "requests_pct": _pct(primary),
                    "tokens_pct": _pct(secondary),
                    "requests_resets_at": _reset_at(primary),
                    "tokens_resets_at": _reset_at(secondary),
                    "timestamp": time.time(),
                    "source": url,
                }
                return _write_openai_cache(payload)
        except urllib.error.HTTPError as error:
            body = error.read(500).decode(errors="replace")
            _log(OPENAI_LOG, f"HTTPError {error.code} from {url}: {body}")
        except (
            urllib.error.URLError,
            TimeoutError,
            OSError,
            ValueError,
            json.JSONDecodeError,
        ) as error:
            _log(OPENAI_LOG, f"fetch failed for {url}: {type(error).__name__}: {error}")

    url = "https://api.openai.com/v1/chat/completions"
    payload = {
        "model": "gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 1,
    }
    _log(OPENAI_LOG, f"fallback requesting {url} to read rate-limit headers")
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode(),
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            headers = response.headers
            fallback_payload = {
                "requests_limit": int(headers.get("x-ratelimit-limit-requests", 0) or 0),
                "requests_remaining": int(headers.get("x-ratelimit-remaining-requests", 0) or 0),
                "tokens_limit": int(headers.get("x-ratelimit-limit-tokens", 0) or 0),
                "tokens_remaining": int(headers.get("x-ratelimit-remaining-tokens", 0) or 0),
                "requests_resets_at": headers.get("x-ratelimit-reset-requests", ""),
                "tokens_resets_at": headers.get("x-ratelimit-reset-tokens", ""),
                "timestamp": time.time(),
                "source": url,
            }
            return _write_openai_cache(fallback_payload)
    except urllib.error.HTTPError as error:
        body = error.read(500).decode(errors="replace")
        _log(OPENAI_LOG, f"fallback HTTPError {error.code}: {body}")
    except (
        urllib.error.URLError,
        TimeoutError,
        OSError,
        ValueError,
        json.JSONDecodeError,
    ) as error:
        _log(OPENAI_LOG, f"fallback fetch failed: {type(error).__name__}: {error}")
    return 1


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main(argv: list[str]) -> int:
    if len(argv) != 2 or argv[1] not in {"fetch-claude", "fetch-openai", "fetch-all"}:
        print("usage: usage_fetch.py {fetch-claude|fetch-openai|fetch-all}", file=sys.stderr)
        return 2

    command = argv[1]
    if command == "fetch-claude":
        return fetch_claude_usage()
    if command == "fetch-openai":
        return fetch_openai_usage()

    claude_status = fetch_claude_usage()
    openai_status = fetch_openai_usage()
    return 0 if claude_status == 0 and openai_status == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
