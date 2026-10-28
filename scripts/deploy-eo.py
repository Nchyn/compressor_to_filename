#!/usr/bin/env python3
"""EdgeOne Upload deployment using only Python 3 standard library."""
import argparse
import getpass
import hashlib
import hmac
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


OPENER = urllib.request.build_opener(NoRedirect())


def request(url, data, headers, method='POST'):
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with OPENER.open(req, timeout=120) as response:
            return response.read()
    except urllib.error.HTTPError as exc:
        # Avoid printing request bodies, upload credentials or signed headers.
        raise RuntimeError('HTTP %s from %s' % (exc.code, urllib.parse.urlsplit(url).hostname)) from None


def api(token, region, action, **fields):
    host = {'global': 'pages-api.edgeone.ai', 'china': 'pages-api.cloud.tencent.com'}[region]
    raw = request('https://' + host + '/v1', json.dumps(dict(Action=action, **fields)).encode(),
                  {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'})
    result = json.loads(raw)
    if result.get('Code') not in (0, '0'):
        raise RuntimeError('%s: API code %s: %s' % (action, result.get('Code'), result.get('Message', 'unknown error')))
    response = result.get('Data', {}).get('Response', {})
    if response.get('Error'):
        raise RuntimeError('%s failed: %s' % (action, response['Error'].get('Code')))
    return response


def cos_authorization(secret_id, secret_key, host, path, timestamp):
    # COS XML API signature v5. Security token is supplied as an unsigned header.
    key_time = '%d;%d' % (timestamp - 60, timestamp + 1800)
    sign_key = hmac.new(secret_key.encode(), key_time.encode(), hashlib.sha1).hexdigest()
    canonical = 'put\n%s\n\nhost=%s\n' % (path, urllib.parse.quote(host, safe='-_.~'))
    string_to_sign = 'sha1\n%s\n%s\n' % (key_time, hashlib.sha1(canonical.encode()).hexdigest())
    signature = hmac.new(sign_key.encode(), string_to_sign.encode(), hashlib.sha1).hexdigest()
    return ('q-sign-algorithm=sha1&q-ak=%s&q-sign-time=%s&q-key-time=%s'
            '&q-header-list=host&q-url-param-list=&q-signature=%s') % (secret_id, key_time, key_time, signature)


def upload(credential, archive):
    bucket = credential.get('Bucket', '')
    if not re.fullmatch(r'[A-Za-z0-9-]+', bucket):
        raise RuntimeError('Invalid upload bucket')
    prefix = credential.get('TargetPath', '').strip('/')
    if not prefix or any(p in ('.', '..') for p in prefix.split('/')):
        raise RuntimeError('Invalid upload prefix')
    key = prefix + '/artifact.zip'
    host = bucket + '.cos.accelerate.myqcloud.com'
    path = '/' + key
    credentials = credential['Credentials']
    authorization = cos_authorization(credentials['TmpSecretId'], credentials['TmpSecretKey'], host, path, int(time.time()))
    request('https://' + host + urllib.parse.quote(path, safe='/-_.~'), archive,
            {'Host': host, 'Authorization': authorization, 'x-cos-security-token': credentials['Token'],
             'Content-Type': 'application/zip', 'Content-MD5': __import__('base64').b64encode(hashlib.md5(archive).digest()).decode()}, 'PUT')
    return key


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', nargs='?', default='preview', choices=['preview', 'production', 'build-only'])
    args = parser.parse_args()
    if args.mode == '--preview':
        args.mode = 'preview'
    project_id = os.environ.get('EO_PROJECT_ID', 'pages-7hlzbzbihmtg')
    region = os.environ.get('EO_REGION', 'global')
    if region not in ('global', 'china'):
        raise RuntimeError('EO_REGION must be global or china')
    if args.mode == 'production' and os.environ.get('EO_CONFIRM_DEPLOY') != '1':
        if not sys.stdin.isatty() or input('Update production %s? Type DEPLOY: ' % project_id) != 'DEPLOY':
            raise RuntimeError('Production deployment not confirmed')
    spec = importlib.util.spec_from_file_location('build_site', Path(__file__).with_name('build-site.py'))
    builder = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(builder)
    directory = builder.build()
    print('Static artifact: ' + str(directory), flush=True)
    if args.mode == 'build-only':
        print('Build only. No upload performed.')
        return
    token = os.environ.get('MAKERS_API_TOKEN', '')
    if not token:
        if not sys.stdin.isatty():
            raise RuntimeError('Set MAKERS_API_TOKEN in CI')
        token = getpass.getpass('EO API Token (hidden): ')
    if not token.strip():
        raise RuntimeError('Empty API token')
    try:
        projects = api(token, region, 'DescribePagesProjects', Filters=[{'Name': 'ProjectId', 'Values': [project_id]}], Offset=0, Limit=100).get('Projects', [])
        project = next((p for p in projects if p.get('ProjectId') == project_id), None)
        if not project or project.get('Provider') != 'Upload':
            raise RuntimeError('Target must be an existing Upload project')
        env = 'Production' if args.mode == 'production' else 'Preview'
        print('Target: %s (%s), %s' % (project['Name'], project_id, env), flush=True)
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, 'w', zipfile.ZIP_DEFLATED) as z:
            for p in sorted(directory.rglob('*')):
                if p.is_file():
                    z.write(p, p.relative_to(directory).as_posix())
        credential = api(token, region, 'DescribePagesCosTempToken', ProjectId=project_id)
        key = upload(credential, buffer.getvalue())
        # Never automatically retry this write: a timeout can mean it succeeded.
        created = api(token, region, 'CreatePagesDeployment', ProjectId=project_id, Env=env,
                      ViaMeta='Upload', Provider='Upload', DistType='Zip', TempBucketPath=key)
        deployment_id = created.get('DeploymentId')
        if not deployment_id:
            raise RuntimeError('Missing deployment ID; inspect console before retrying')
        print('Deployment ID: ' + deployment_id, flush=True)
        deadline = time.monotonic() + 600
        previous = None
        while time.monotonic() < deadline:
            result = api(token, region, 'DescribePagesDeployments', ProjectId=project_id, DeploymentIds=[deployment_id], Offset=0, Limit=1)
            deployments = result.get('Deployments', [])
            deployment = next((d for d in deployments if d.get('DeploymentId') == deployment_id), None)
            if deployment:
                status = deployment.get('Status')
                if status != previous:
                    print('Status: ' + str(status), flush=True)
                    previous = status
                if status == 'Success':
                    print('Deployment successful. URL: ' + str(deployment.get('PreviewUrl') or '(see EO console)'))
                    return
                if status in ('Fail', 'Failed', 'Canceled', 'Cancelled', 'Error'):
                    raise RuntimeError('Deployment failed: ' + status)
            time.sleep(5)
        raise RuntimeError('Polling timed out; deployment may still be running. Do not upload again without checking ' + deployment_id)
    except Exception as exc:
        raise RuntimeError(str(exc).replace(token, '[REDACTED]')) from None


if __name__ == '__main__':
    try:
        main()
    except (Exception, KeyboardInterrupt) as exc:
        print('Deployment stopped: ' + str(exc), file=sys.stderr)
        sys.exit(1)
