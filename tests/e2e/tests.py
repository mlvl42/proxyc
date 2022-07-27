import pytest
import os
import subprocess


CONTAINER_IP=os.environ.get('CONTAINER_IP')
PROXYC=os.environ.get('TARGET_BIN')


def execute(cmd, encoding='UTF-8', timeout=None, shell=False):
    """Execute a shell command/binary.
    Arguments:
    cmd:        List[str] -- splitted command (ex: ['ls', '-la', '~'])
    encoding:   str (default: 'UTF-8') -- used for decoding the command output
    timeout:    int (default: None) -- in seconds, raises TimeoutExpired if the
    result: a tuple coontaining stdout, the returncode and stderr
    """

    print(' '.join(cmd))
    proc = subprocess.Popen(cmd, stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE, stderr=subprocess.PIPE, shell=shell)
    output, error = proc.communicate(timeout=timeout)
    output = output.decode(encoding).rstrip()
    error = error.decode(encoding).rstrip()
    rc = proc.returncode
    print(error)
    return (output, rc, error)


def test_socks5_simple():
    out, rc, err = execute([
        PROXYC,
        f'--proxy=socks5://{CONTAINER_IP}:1080',
        'curl',
        'http://127.0.0.1:8000'])
    assert out == 'OK'
    assert rc == 0

def test_socks5_auth():
    out, rc, err = execute([
        PROXYC,
        f'--proxy=socks5://admin:password@{CONTAINER_IP}:1081',
        'curl',
        'http://127.0.0.1:8000'])
    assert out == 'OK'
    assert rc == 0

def test_http_simple():
    out, rc, err = execute([
        PROXYC,
        f'--proxy=http://{CONTAINER_IP}:8888',
        'curl',
        'http://127.0.0.1:8000'])
    assert out == 'OK'
    assert rc == 0

def test_socks5_badauth():
    out, rc, err = execute([
        PROXYC,
        f'--proxy=socks5://admi:password@{CONTAINER_IP}:1081',
        'curl',
        'http://127.0.0.1:8000'])
    assert rc != 0


def test_chain_socks():
    out, rc, err = execute([
        PROXYC,
        f'--proxy=socks5://{CONTAINER_IP}:1080',
        f'--proxy=socks5://admin:password@127.0.0.1:1081',
        'curl',
        'http://127.0.0.1:8000'])
    assert out == 'OK'
    assert rc == 0

def test_chain_socks_http():
    out, rc, err = execute([
        PROXYC,
        f'--proxy=socks5://{CONTAINER_IP}:1080',
        f'--proxy=http://127.0.0.1:8888',
        'curl',
        'http://127.0.0.1:8000'])
    assert out == 'OK'
    assert rc == 0

def test_chain_socks_http():
    out, rc, err = execute([
        PROXYC,
        f'--proxy=socks5://{CONTAINER_IP}:1080',
        f'--proxy=http://127.0.0.1:8888',
        'curl',
        'http://127.0.0.1:8000'])
    assert out == 'OK'
    assert rc == 0
