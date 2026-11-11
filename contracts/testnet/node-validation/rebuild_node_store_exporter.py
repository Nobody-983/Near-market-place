#!/usr/bin/env python3
"""Export retained stopped-node receipts and independently replay copied evidence.

No node is started, no keys are accepted, and no case passes merely because a
case name or database exists. Build store_exporter.rs at the pinned source first.
"""
import argparse
import fcntl
import shutil
import subprocess
import tempfile
from contextlib import ExitStack
from common import *
from store_decode import read_export
import verify


def inventory(root):
    """Refuse links/special files; hash only the public DB, streaming large SSTs."""
    result = {}
    for path in sorted(root.rglob('*')):
        require(not path.is_symlink(), 'symlink in stopped database')
        if path.is_dir():
            continue
        require(path.is_file(), 'special file in stopped database')
        require(path.name not in ('node_key.json', 'validator_key.json'), 'key file in database')
        with path.open('rb') as f:
            checksum = hashlib.file_digest(f, 'sha256').hexdigest()
        result[str(path.relative_to(root))] = {'bytes': path.stat().st_size, 'sha256': checksum}
    require('CURRENT' in result and 'LOCK' in result, 'not a RocksDB directory')
    return result


def export_command(stage, hot, cold):
    command = ['bwrap', '--unshare-all', '--die-with-parent', '--new-session',
        '--ro-bind', '/usr', '/usr', '--symlink', 'usr/bin', '/bin',
        '--symlink', 'usr/lib', '/lib', '--symlink', 'usr/lib64', '/lib64',
        '--proc', '/proc', '--dev', '/dev', '--tmpfs', '/tmp', '--dir', '/run', '--dir', '/db',
        '--ro-bind', str(stage/'isolated'), '/run/nft-validation-isolated',
        '--ro-bind', str(stage/'identity.json'), '/opt/identity.json',
        '--ro-bind', str(stage/'exporter'), '/opt/exporter', '--ro-bind', str(hot), '/db/data']
    if cold:
        command += ['--ro-bind', str(cold), '/db/cold-data']
    return command + ['--clearenv', '--setenv', 'PATH', '/usr/bin:/bin',
                      '--chdir', '/tmp', '--', '/opt/exporter']


def augment(evidence, receipts, locations):
    # Deep-copy: original execution evidence remains immutable.
    e = json.loads(json.dumps(evidence))
    required = {rid for row in verify.outcomes(e['result']).values() for rid in row['outcome']['receipt_ids']}
    for rid in required:
        if rid not in receipts:
            continue
        if rid in e['bodies']:
            require(e['bodies'][rid] == receipts[rid], 'RPC/store receipt conflict')
        else:
            e['bodies'][rid] = receipts[rid]
    data_ids = {d['data_id'] for rid, body in e['bodies'].items() if rid in required and verify.action_body(body)
                for d in verify.action_body(body).get('output_data_receivers', [])}
    data = e.setdefault('data_receipts', {})
    for rid, body in receipts.items():
        value = body['receipt'].get('Data')
        if value is None or value['data_id'] not in data_ids:
            continue
        if rid in data:
            require(data[rid] == body, 'RPC/store DataReceipt conflict')
        data[rid] = body
    seen = {b['receipt']['Data']['data_id'] for b in data.values()}
    e['unobserved_data_ids'] = sorted(data_ids-seen)
    e['store_receipt_locations'] = {rid: locations[rid] for rid in required | data.keys() if rid in locations}
    e['store_export_note'] = ('Borsh-decoded stored bodies; GC markers excluded from processing locations. '
                              'No delivery time inferred from mere storage.')
    return e


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--exporter-bin', type=Path, required=True)
    p.add_argument('--exporter-sha256', required=True)
    p.add_argument('--case-dir', type=Path, required=True,
                   help='variant directory containing environment.json and stopped-node-data')
    p.add_argument('--output', type=Path, required=True, help='fresh export/replay directory')
    a = p.parse_args()
    require(not a.output.exists(), 'output must be fresh')
    require(a.exporter_bin.is_file() and digest(a.exporter_bin) == a.exporter_sha256, 'exporter binary SHA-256 mismatch')
    case = a.case_dir.resolve()
    environment = load(case/'environment.json')
    require(environment['evidence_class'] == 'standalone-sandbox', 'export requires a stopped Sandbox variant')
    status = environment['status']
    require(status['version']['version'] == VERSION and status['version']['commit'] == COMMIT
            and status['protocol_version'] == PROTOCOL, 'stopped node pin mismatch')
    require(status['chain_id'].startswith('nft-validation-local-'), 'local chain identity required')
    hot, cold = case/'stopped-node-data', case/'stopped-node-cold-data'
    require(hot.is_dir() and not hot.is_symlink(), 'stopped node store missing or linked')
    cold = cold if cold.is_dir() else None
    require(cold is None or not cold.is_symlink(), 'linked cold database')
    a.output.mkdir(parents=True)
    output = a.output.resolve()
    with ExitStack() as stack:
        # POSIX write lock fails when a live node owns the store. Retain it during
        # export; native code opens read-only and never requests the writer lock.
        for db in (hot, cold):
            if db is None:
                continue
            require((db/'LOCK').is_file() and not (db/'LOCK').is_symlink(), 'database lock missing or linked')
            lock = stack.enter_context((db/'LOCK').open('r+b'))
            fcntl.lockf(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        before = {name: inventory(db) for name, db in [('hot', hot), ('cold', cold)] if db}
        with tempfile.TemporaryDirectory(prefix='nft-store-export-') as tmp:
            stage = Path(tmp)
            (stage/'isolated').write_text('read-only stopped store export\n')
            save(stage/'identity.json',environment['identity'])
            shutil.copyfile(a.exporter_bin, stage/'exporter')
            (stage/'exporter').chmod(0o555)
            require(digest(stage/'exporter') == a.exporter_sha256, 'staged exporter changed')
            with (output/'records.jsonl').open('x') as stdout, (output/'exporter.log').open('x') as stderr:
                result = subprocess.run(export_command(stage, hot, cold), stdout=stdout, stderr=stderr, timeout=300)
            require(result.returncode == 0, 'isolated store exporter failed; see exporter.log')
        after = {name: inventory(db) for name, db in [('hot', hot), ('cold', cold)] if db}
        require(before == after, 'stopped database changed during read-only export')
    save(output/'provenance.json', {'source_commit': COMMIT, 'exporter_sha256': a.exporter_sha256,
        'records_sha256': digest(output/'records.jsonl'), 'database_before': before, 'database_after': after,
        'environment': environment, 'environment_sha256': digest(case/'environment.json'),
        'scope': 'local store consistency; not public-chain authenticity or a case verdict'})
    receipts, locations = read_export(output/'records.jsonl',environment['identity'])
    checks = []
    for source in sorted(case.glob('*.json')):
        original = load(source)
        if not isinstance(original, dict) or not {'result', 'before', 'after', 'submitted'} <= original.keys():
            continue
        target = output/source.name
        try:
            augmented = augment(original, receipts, locations)
            augmented['source_evidence_sha256'] = digest(source)
            save(target, augmented)
            checks.append({'file': source.name, 'status': 'PASS', 'checks': verify.verify_step(target)})
        except Inconclusive as exc:
            checks.append({'file': source.name, 'status': 'INCONCLUSIVE', 'reason': str(exc)})
        except (ValueError, KeyError, TypeError) as exc:
            checks.append({'file': source.name, 'status': 'FAIL', 'reason': str(exc)})
    require(checks, 'no transaction evidence to replay')
    save(output/'verification.json', {'schema': 1,
         'scope': 'transaction evidence only; original case verdict unchanged', 'checks': checks})
    return 0 if all(row['status'] == 'PASS' for row in checks) else 1


if __name__ == '__main__':
    raise SystemExit(main())
