#!/usr/bin/env python3
"""Install the test-only overlay into a fresh verified nearcore source archive."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
from source_tree import ARCHIVE_SHA, COMMIT, extract, verify_tree

def prepare(archive, output):
    root = extract(archive,output)
    target = root / 'test-loop-tests'
    overlay = Path(__file__).parent / 'testloop/nft_validation.rs'
    shutil.copyfile(overlay, target / 'src/nft_validation.rs')
    (target / 'src/nft_validation_lib.rs').write_text('#![cfg(test)]\nmod setup;\nmod utils;\nmod nft_validation;\n')
    # Compile official node/setup utilities plus this adapter; avoid compiling
    # unrelated integration-test modules. No production/runtime source changes.
    with (target / 'Cargo.toml').open('a') as f:
        f.write('\n[lib]\npath = "src/nft_validation_lib.rs"\n')
    exporter=root/'core/store/src/bin/nft-receipt-exporter.rs'
    exporter.parent.mkdir(parents=True,exist_ok=True)
    shutil.copyfile(Path(__file__).parent/'store_exporter.rs',exporter)
    changes=['test-loop-tests/Cargo.toml','test-loop-tests/src/nft_validation.rs',
             'test-loop-tests/src/nft_validation_lib.rs','core/store/src/bin/nft-receipt-exporter.rs']
    overlay_files={name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in changes}
    verified=verify_tree(archive,root,overlay_files)
    provenance={'source_commit':COMMIT,'archive_sha256':ARCHIVE_SHA,'overlay_sha256':hashlib.sha256(overlay.read_bytes()).hexdigest(),
                'changes':changes,'overlay_files':overlay_files,'verified_tree':verified,
                'compilation_status':'NOT_RUN','runtime_status':'NOT_RUN'}
    (root/'nft-validation-source.json').write_text(json.dumps(provenance,indent=2)+'\n')
    print(root)

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('archive',type=Path); p.add_argument('output',type=Path)
    a=p.parse_args(); prepare(a.archive,a.output)
