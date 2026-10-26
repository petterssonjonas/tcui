"""Offline tests for notify-ping credential selection and redaction."""
import contextlib
import importlib.machinery
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.error


loader = importlib.machinery.SourceFileLoader("notify_ping_under_test", str(Path(__file__).with_name("notify-ping")))
spec = importlib.util.spec_from_loader(loader.name, loader)
notify = importlib.util.module_from_spec(spec)
sys.modules[loader.name] = notify
loader.exec_module(notify)
USER = "u" * 30
APP = "a" * 30
OTHER = "b" * 30


class NotifyPingTests(unittest.TestCase):
    def test_explicit_flags_override_environment_and_private_file(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "credentials.env"
            path.write_text(f"PUSHOVER_USER_KEY={OTHER}\nPUSHOVER_TCUI_APP_TOKEN={OTHER}\n", encoding="utf-8")
            path.chmod(0o600)
            with patch.dict(os.environ, {"PUSHOVER_USER_KEY": OTHER, "PUSHOVER_TCUI_APP_TOKEN": OTHER}, clear=True):
                args = notify.parser().parse_args(["--env-file", str(path), "--app", "tcui", "--user-key", USER, "--app-key", APP, "message"])
                payload = notify.build_payload(args)
            self.assertEqual((payload["user"], payload["token"]), (USER, APP))

    def test_empty_file_override_disables_ambient_file_read_and_dry_run_redacts(self):
        environment = {"PUSHOVER_USER_KEY": USER, "PUSHOVER_TCUI_APP_TOKEN": APP, "PUSHOVER_ENV_FILE": ""}
        output = io.StringIO()
        with patch.dict(os.environ, environment, clear=True), patch.object(sys, "argv", ["notify-ping", "--app", "tcui", "--dry-run", "message"]), patch.object(notify, "read_env_file", side_effect=AssertionError("ambient file read")), patch.object(notify, "send", side_effect=AssertionError("network send")), contextlib.redirect_stdout(output):
            self.assertEqual(notify.main(), 0)
        payload = json.loads(output.getvalue())
        self.assertEqual((payload["user"], payload["token"]), ("<redacted>", "<redacted>"))
        self.assertNotIn(USER, output.getvalue())
        self.assertNotIn(APP, output.getvalue())

    def test_private_file_rejects_permissions_symlinks_shell_and_oversize(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "credentials.env"
            path.write_text(f"PUSHOVER_USER_KEY={USER}\n", encoding="utf-8")
            path.chmod(0o644)
            with self.assertRaises(notify.CliError):
                notify.read_env_file(path)
            path.chmod(0o600)
            self.assertEqual(notify.read_env_file(path)["PUSHOVER_USER_KEY"], USER)
            link = Path(root) / "link"
            link.symlink_to(path)
            with self.assertRaises(notify.CliError):
                notify.read_env_file(link)
            for text in ["PUSHOVER_USER_KEY=$(id)", "PUSHOVER_USER_KEY=`id`", "PUSHOVER_USER_KEY='unterminated", "a" * 65_537]:
                path.write_text(text, encoding="utf-8")
                with self.assertRaises(notify.CliError):
                    notify.read_env_file(path)

    def test_invalid_key_error_never_contains_value(self):
        with patch.dict(os.environ, {"PUSHOVER_ENV_FILE": ""}, clear=True):
            args = notify.parser().parse_args(["--user-key", "invalid-secret-canary", "--app-key", APP, "message"])
            with self.assertRaises(notify.CliError) as failure:
                notify.build_payload(args)
        self.assertNotIn("invalid-secret-canary", str(failure.exception))

    def test_service_failure_does_not_echo_response_credentials(self):
        error = urllib.error.HTTPError(notify.API_URL, 400, "rejected", {}, io.BytesIO(f"user={USER}&token={APP}".encode()))
        with patch.object(notify.urllib.request, "urlopen", side_effect=error):
            with self.assertRaises(notify.CliError) as failure:
                notify.send({"user": USER, "token": APP, "message": "test"})
        self.assertNotIn(USER, str(failure.exception))
        self.assertNotIn(APP, str(failure.exception))

    def test_shared_dotenv_preserves_custom_token_profiles_and_literal_titles(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "credentials.env"
            path.write_text(
                f"OTHER_APP_OPTIONS=$(not-executed)\n"
                f"PUSHOVER_USER_KEY={USER}\nCUSTOM_APP_TOKEN={APP}\n"
                "PUSHOVER_OMP_TITLE=\"Jonas's $literal; title\"\n",
                encoding="utf-8",
            )
            path.chmod(0o600)
            with patch.dict(os.environ, {}, clear=True):
                args = notify.parser().parse_args(["--env-file", str(path), "--app", "omp", "--token-env", "CUSTOM_APP_TOKEN", "message"])
                payload = notify.build_payload(args)
            self.assertEqual(payload["title"], "Jonas's $literal; title")
            self.assertEqual((payload["user"], payload["token"]), (USER, APP))


if __name__ == "__main__":
    unittest.main()
