"""Generate a Lovelace SRT when an OBS recording finishes."""

import os
from pathlib import Path
import subprocess
import tempfile

import obspython as obs


_executable = ""
_working_directory = ""
_language = "pt"
_track = 1
_enabled = True
_jobs = []


def script_description():
    return "Generate an SRT next to a finished OBS recording using Lovelace and FFmpeg."


def script_defaults(settings):
    obs.obs_data_set_default_bool(settings, "enabled", True)
    obs.obs_data_set_default_string(settings, "language", "pt")
    obs.obs_data_set_default_int(settings, "track", 1)


def script_properties():
    props = obs.obs_properties_create()
    obs.obs_properties_add_bool(props, "enabled", "Generate captions after recording")
    obs.obs_properties_add_path(
        props, "executable", "Lovelace CLI", obs.OBS_PATH_FILE, "", ""
    )
    obs.obs_properties_add_path(
        props, "working_directory", "Folder containing engines and models (optional)",
        obs.OBS_PATH_DIRECTORY, "", ""
    )
    language = obs.obs_properties_add_list(
        props, "language", "Spoken language", obs.OBS_COMBO_TYPE_LIST, obs.OBS_COMBO_FORMAT_STRING
    )
    for label, value in (("Portuguese", "pt"), ("English", "en"), ("Auto-detect", "auto")):
        obs.obs_property_list_add_string(language, label, value)
    obs.obs_properties_add_int(props, "track", "Audio track in the file", 1, 6, 1)
    return props


def script_update(settings):
    global _executable, _working_directory, _language, _track, _enabled
    _executable = obs.obs_data_get_string(settings, "executable")
    _working_directory = obs.obs_data_get_string(settings, "working_directory")
    _language = obs.obs_data_get_string(settings, "language")
    _track = obs.obs_data_get_int(settings, "track")
    _enabled = obs.obs_data_get_bool(settings, "enabled")


def script_load(settings):
    if not hasattr(obs, "obs_frontend_get_last_recording"):
        obs.script_log(obs.LOG_ERROR, "Lovelace captions require OBS Studio 29 or later.")
        return
    obs.obs_frontend_add_event_callback(_on_event)
    obs.timer_add(_check_jobs, 1000)


def script_unload():
    if hasattr(obs, "obs_frontend_get_last_recording"):
        obs.obs_frontend_remove_event_callback(_on_event)
        obs.timer_remove(_check_jobs)
    for _, log, _ in _jobs:
        log.close()
    _jobs.clear()


def _on_event(event):
    if event != obs.OBS_FRONTEND_EVENT_RECORDING_STOPPED or not _enabled:
        return
    recording = obs.obs_frontend_get_last_recording()
    if not recording:
        obs.script_log(obs.LOG_WARNING, "OBS did not report a recording file.")
        return
    if not _executable or not Path(_executable).is_file():
        obs.script_log(obs.LOG_ERROR, "Select the Lovelace CLI executable in the script settings.")
        return

    executable = Path(_executable).resolve()
    working_directory = Path(_working_directory) if _working_directory else executable.parent
    media = Path(recording).absolute()
    log = None
    try:
        log = tempfile.TemporaryFile()
        job = subprocess.Popen(
            [str(executable), "captions", str(media), "--language", _language,
             "--audio-track", str(_track)],
            cwd=working_directory,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
        )
    except OSError as error:
        if log is not None:
            log.close()
        obs.script_log(obs.LOG_ERROR, "Could not start Lovelace: {}".format(error))
        return
    _jobs.append((job, log, media))
    obs.script_log(obs.LOG_INFO, "Generating captions for {}".format(media))


def _check_jobs():
    for entry in _jobs[:]:
        job, log, media = entry
        result = job.poll()
        if result is None:
            continue
        _jobs.remove(entry)
        with log:
            if result == 0:
                obs.script_log(obs.LOG_INFO, "Captions saved to {}".format(media.with_suffix(".srt")))
            else:
                end = log.seek(0, os.SEEK_END)
                log.seek(max(0, end - 4096))
                details = log.read().decode("utf-8", errors="replace").strip()
                obs.script_log(obs.LOG_ERROR, "Captions failed for {}\n{}".format(media, details))
