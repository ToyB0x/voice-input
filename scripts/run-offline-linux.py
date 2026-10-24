#!/usr/bin/env python3
"""Execute a child with network syscalls denied by inherited Linux seccomp rules."""
import ctypes
import errno
import os
import sys


def main():
    if sys.platform != 'linux' or len(sys.argv) < 2:
        raise SystemExit('usage (Linux only): run-offline-linux.py COMMAND [ARGS...]')
    library = ctypes.CDLL('libseccomp.so.2', use_errno=True)
    library.seccomp_init.argtypes = [ctypes.c_uint32]
    library.seccomp_init.restype = ctypes.c_void_p
    library.seccomp_syscall_resolve_name.argtypes = [ctypes.c_char_p]
    library.seccomp_syscall_resolve_name.restype = ctypes.c_int
    library.seccomp_rule_add.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_int, ctypes.c_uint]
    library.seccomp_rule_add.restype = ctypes.c_int
    library.seccomp_load.argtypes = [ctypes.c_void_p]
    library.seccomp_load.restype = ctypes.c_int
    library.seccomp_release.argtypes = [ctypes.c_void_p]
    context = library.seccomp_init(0x7fff0000)  # SCMP_ACT_ALLOW
    if not context:
        raise RuntimeError('Cannot initialize network sandbox')
    try:
        # Include io_uring so it cannot submit network operations around the
        # ordinary socket syscall restrictions. Rules survive exec and fork.
        names = ['socket', 'socketpair', 'connect', 'bind', 'listen', 'accept', 'accept4',
                 'sendto', 'sendmsg', 'sendmmsg', 'recvfrom', 'recvmsg', 'recvmmsg',
                 'shutdown', 'getsockname', 'getpeername', 'setsockopt', 'getsockopt',
                 'io_uring_setup', 'io_uring_enter', 'io_uring_register']
        for name in names:
            number = library.seccomp_syscall_resolve_name(name.encode())
            if number < 0 or library.seccomp_rule_add(context, 0x50000 | errno.EPERM, number, 0) != 0:
                raise RuntimeError('Cannot configure network sandbox')
        if library.seccomp_load(context) != 0:
            raise RuntimeError('Cannot enforce network sandbox')
    finally:
        library.seccomp_release(context)
    os.execvp(sys.argv[1], sys.argv[1:])


if __name__ == '__main__':
    main()
