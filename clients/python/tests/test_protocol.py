import json
from pathlib import Path

import pytest

from cachebox import protocol


ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "fixtures" / "native-protocol" / "v1" / "frames.json"


ERRORS = {
    "BadMagic": protocol.BadMagic,
    "UnsupportedVersion": protocol.UnsupportedVersion,
    "InvalidKind": protocol.InvalidKind,
    "UnknownCommand": protocol.UnknownCommand,
    "NonzeroFlags": protocol.NonzeroFlags,
    "NonzeroReserved": protocol.NonzeroReserved,
    "FrameTooLarge": protocol.FrameTooLarge,
    "TrailingBytes": protocol.TrailingBytes,
    "InvalidNamespace": protocol.InvalidNamespace,
    "InvalidTag": protocol.InvalidTag,
    "InvalidBool": protocol.InvalidBool,
    "InvalidPayload": protocol.InvalidPayload,
}


def load_fixtures():
    with FIXTURES.open("r", encoding="utf-8") as file:
        return json.load(file)["fixtures"]


@pytest.mark.parametrize("fixture", load_fixtures(), ids=lambda item: item["name"])
def test_shared_protocol_fixtures(fixture):
    frame = bytes.fromhex(fixture["hex"])
    direction = fixture["direction"]

    if direction == "request":
        decoded = protocol.decode_request_frame(frame, max_payload_len=1024)

        assert protocol.encode_request_frame(decoded) == frame
        assert decoded.request_id == fixture["request_id"]
        assert decoded.command.name == command_name(fixture["command"])
    elif direction == "response":
        decoded = protocol.decode_response_frame(frame, max_payload_len=1024)

        assert protocol.encode_response_frame(decoded) == frame
        assert decoded.request_id == fixture["request_id"]
        assert decoded.command.name == command_name(fixture["command"])
    elif direction == "malformed_request":
        with pytest.raises(ERRORS[fixture["expected_error"]]):
            protocol.decode_request_frame(frame, max_payload_len=1024)
    elif direction == "malformed_response":
        with pytest.raises(ERRORS[fixture["expected_error"]]):
            protocol.decode_response_frame(frame, max_payload_len=1024)
    else:
        raise AssertionError(f"unknown fixture direction {direction!r}")


def command_name(value):
    words = []
    start = 0
    for index, char in enumerate(value):
        if index > 0 and char.isupper():
            words.append(value[start:index])
            start = index
    words.append(value[start:])
    return "_".join(words).upper()
