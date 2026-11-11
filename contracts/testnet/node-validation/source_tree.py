"""Checksum and file-level provenance for the exact nearcore archive and overlays."""
import tarfile
from common import *

ARCHIVE_SHA = '3c4254540a4bac8edf07c49ea07fcaf0bcbb59fe51afd2e37bad6ff036db3f99'

def archive_files(archive):
    require(digest(archive)==ARCHIVE_SHA,'pinned nearcore source archive checksum mismatch')
    files={}; prefix='nearcore-'+COMMIT+'/'
    with tarfile.open(archive) as source:
        for member in source:
            require(member.name==prefix[:-1] or member.name.startswith(prefix),'unexpected archive root')
            name=member.name[len(prefix):]
            if member.isdir(): continue
            require(name and '..' not in Path(name).parts and name not in files,'unsafe/duplicate archive path')
            if member.issym():
                require(not Path(member.linkname).is_absolute(),'absolute archive link')
                files[name]='symlink:'+member.linkname
            else:
                require(member.isfile(),'unexpected special archive member')
                files[name]=sha(source.extractfile(member).read()).hex()
    return files

def verify_tree(archive,root,overlay=None):
    expected=archive_files(archive)
    expected.update(overlay or {})
    for name,checksum in expected.items():
        path=root/name
        if checksum.startswith('symlink:'):
            require(path.is_symlink() and str(path.readlink())==checksum[8:] and path.resolve().is_relative_to(root.resolve()),'source link mismatch: '+name)
        else:
            require(path.is_file() and not path.is_symlink() and digest(path)==checksum,'source file mismatch: '+name)
    # An extra build.rs/module/config could affect compilation despite all
    # original files matching. Only generated build output and Git are exempt.
    for path in root.rglob('*'):
        relative=path.relative_to(root)
        if relative.parts[0] in ('.git','target'): continue
        if path.is_file() or path.is_symlink():
            require(str(relative) in expected or str(relative)=='nft-validation-source.json','unexpected source file: '+str(relative))
    return {'archive_sha256':ARCHIVE_SHA,'source_commit':COMMIT,'files':len(expected),
            'tree_manifest_sha256':sha(compact(expected).encode()).hex()}

def extract(archive,output):
    archive_files(archive)  # validate before creating output
    output.mkdir(parents=True,exist_ok=False)
    with tarfile.open(archive) as source: source.extractall(output,filter='data')
    return output/('nearcore-'+COMMIT)
