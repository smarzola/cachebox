from pathlib import Path

import cachebox


ROOT = Path(__file__).resolve().parents[3]


def test_package_imports_with_version():
    assert cachebox.__version__ == "0.2.0"


def test_native_protocol_fixtures_exist_for_codec_milestone():
    fixtures = ROOT / "fixtures" / "native-protocol" / "v1" / "frames.json"

    assert fixtures.exists()
