#!/usr/bin/env python3
"""Fixed synthetic helper protocol checks; never read user audio or screen data."""
import argparse
import json
from pathlib import Path
import socket
import subprocess

AVAILABILITY = {'available', 'osTooOld', 'frameworkMissingFromSDK', 'deviceNotEligible',
                'appleIntelligenceNotEnabled', 'modelNotReady', 'unsupportedLanguageOrLocale',
                'unknownUnavailableReason', 'unknownAvailability'}


def invoke(binary, args, payload=''):
    result = subprocess.run([str(binary.resolve()), *args], input=payload, text=True, capture_output=True, timeout=150)
    # Do not save/print native stderr or a generated response, even for CI fixtures.
    report = json.loads(result.stdout)
    return result.returncode, report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--require-framework', action='store_true')
    args = parser.parse_args()
    try:
        socket.create_connection(('127.0.0.1', 9), timeout=1)
    except PermissionError:
        pass
    else:
        raise ValueError('Expected network sandbox EPERM')
    code, status = invoke(args.binary, ['--status'])
    assert code == 0 and status['protocol_version'] == 1 and status['availability'] in AVAILABILITY
    assert not args.require_framework or status['framework_imported'] is True
    assert set(status) == {'protocol_version', 'framework_imported', 'os_supported', 'availability', 'os_major', 'os_minor', 'os_patch'}
    profiles = ['legacy-structured-v1', 'grounded-structured-v2', 'grounded-text-v2', 'screen-reading-v3']
    code, contract = invoke(args.binary, ['--self-test'])
    assert code == 0 and contract['status'] == 'passed' and contract['profiles'] == profiles
    for profile in profiles:
        code, empty = invoke(args.binary, ['--correct'], json.dumps(dict(transcript='', context='', profile=profile)))
        assert code == 0 and empty['text'] == '' and empty['elapsed_ms'] == 0
        assert empty['protocol']['profile'] == profile
        assert empty['protocol']['output_token_limit'] == (512 if profile == profiles[0] else 96)
    code, invalid_profile = invoke(args.binary, ['--correct'], json.dumps(dict(transcript='', context='', profile='invalid')))
    assert code != 0 and invalid_profile['stage'] == 'input'
    for payload in ['not-json', 'x' * 24577, json.dumps(dict(transcript='x' * 8193, context='')),
                    json.dumps(dict(transcript='ok', context='x' * 6145)), json.dumps(dict(transcript='PRIVATE-CANARY\0', context=''))]:
        code, error = invoke(args.binary, ['--correct'], payload)
        assert code != 0 and error['stage'] == 'input' and 'PRIVATE-CANARY' not in json.dumps(error)
    inference = status['availability'] == 'available'
    for profile in profiles:
        code, report = invoke(args.binary, ['--correct'], json.dumps(dict(transcript='会議は金曜日です', context='会議、金曜日。', profile=profile), ensure_ascii=False))
        if inference:
            assert code == 0 and report['backend'] == 'foundation' and report['offloaded_layers'] == 0 and report['text']
            assert report['protocol']['profile'] == profile
        else:
            assert code != 0 and report['stage'] == 'availability' and report['reason'] == status['availability']
    evidence = dict(status='passed', networkDenied=True, availability=status, inferenceTested=inference,
                    guardTested=False, invalidInputRejected=True, emptyInputBypassed=True, profiles=profiles, promptContractTested=True,
                    limitation='Helper protocol only. Unsupported hosts do not verify model inference; product applies Rust guards separately.')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(evidence, indent=2))


if __name__ == '__main__':
    main()
