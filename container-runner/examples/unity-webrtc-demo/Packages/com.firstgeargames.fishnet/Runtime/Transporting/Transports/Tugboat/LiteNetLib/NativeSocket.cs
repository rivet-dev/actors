using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Sockets;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace LiteNetLib
{
    internal static class NativeSocket
    {
        private static unsafe class WinSock
        {
            private const string LibName = "ws2_32.dll";

            [DllImport(LibName, SetLastError = true)]
            public static extern int recvfrom(IntPtr socketHandle, [In] [Out] byte[] pinnedBuffer, [In] int len, [In] SocketFlags socketFlags, [Out] byte[] socketAddress, [In] [Out] ref int socketAddressSize);

            [DllImport(LibName, SetLastError = true)]
            internal static extern int sendto(IntPtr socketHandle, byte* pinnedBuffer, [In] int len, [In] SocketFlags socketFlags, [In] byte[] socketAddress, [In] int socketAddressSize);
        }

        private static unsafe class UnixSock
        {
            private const string LibName = "libc";

            [DllImport(LibName, SetLastError = true)]
            public static extern int recvfrom(IntPtr socketHandle, [In] [Out] byte[] pinnedBuffer, [In] int len, [In] SocketFlags socketFlags, [Out] byte[] socketAddress, [In] [Out] ref int socketAddressSize);

            [DllImport(LibName, SetLastError = true)]
            internal static extern int sendto(IntPtr socketHandle, byte* pinnedBuffer, [In] int len, [In] SocketFlags socketFlags, [In] byte[] socketAddress, [In] int socketAddressSize);
        }

        public static readonly bool IsSupported = false;
        public static readonly bool UnixMode = false;
        public const int IPv4AddrSize = 16;
        public const int IPv6AddrSize = 28;
        public const int AF_INET = 2;
        public const int AF_INET6 = 10;
        private static readonly Dictionary<int, SocketError> NativeErrorToSocketError = new()
        {
            { 13, SocketError.AccessDenied }, // EACCES
            { 98, SocketError.AddressAlreadyInUse }, // EADDRINUSE
            { 99, SocketError.AddressNotAvailable }, // EADDRNOTAVAIL
            { 97, SocketError.AddressFamilyNotSupported }, // EAFNOSUPPORT
            { 11, SocketError.WouldBlock }, // EAGAIN
            { 114, SocketError.AlreadyInProgress }, // EALREADY
            { 9, SocketError.OperationAborted }, // EBADF
            { 125, SocketError.OperationAborted }, // ECANCELED
            { 103, SocketError.ConnectionAborted }, // ECONNABORTED
            { 111, SocketError.ConnectionRefused }, // ECONNREFUSED
            { 104, SocketError.ConnectionReset }, // ECONNRESET
            { 89, SocketError.DestinationAddressRequired }, // EDESTADDRREQ
            { 14, SocketError.Fault }, // EFAULT
            { 112, SocketError.HostDown }, // EHOSTDOWN
            { 6, SocketError.HostNotFound }, // ENXIO
            { 113, SocketError.HostUnreachable }, // EHOSTUNREACH
            { 115, SocketError.InProgress }, // EINPROGRESS
            { 4, SocketError.Interrupted }, // EINTR
            { 22, SocketError.InvalidArgument }, // EINVAL
            { 106, SocketError.IsConnected }, // EISCONN
            { 24, SocketError.TooManyOpenSockets }, // EMFILE
            { 90, SocketError.MessageSize }, // EMSGSIZE
            { 100, SocketError.NetworkDown }, // ENETDOWN
            { 102, SocketError.NetworkReset }, // ENETRESET
            { 101, SocketError.NetworkUnreachable }, // ENETUNREACH
            { 23, SocketError.TooManyOpenSockets }, // ENFILE
            { 105, SocketError.NoBufferSpaceAvailable }, // ENOBUFS
            { 61, SocketError.NoData }, // ENODATA
            { 2, SocketError.AddressNotAvailable }, // ENOENT
            { 92, SocketError.ProtocolOption }, // ENOPROTOOPT
            { 107, SocketError.NotConnected }, // ENOTCONN
            { 88, SocketError.NotSocket }, // ENOTSOCK
            { 3440, SocketError.OperationNotSupported }, // ENOTSUP
            { 1, SocketError.AccessDenied }, // EPERM
            { 32, SocketError.Shutdown }, // EPIPE
            { 96, SocketError.ProtocolFamilyNotSupported }, // EPFNOSUPPORT
            { 93, SocketError.ProtocolNotSupported }, // EPROTONOSUPPORT
            { 91, SocketError.ProtocolType }, // EPROTOTYPE
            { 94, SocketError.SocketNotSupported }, // ESOCKTNOSUPPORT
            { 108, SocketError.Disconnecting }, // ESHUTDOWN
            { 110, SocketError.TimedOut }, // ETIMEDOUT
            { 0, SocketError.Success }
        };

        static NativeSocket()
        {
            if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
            {
                IsSupported = true;
                UnixMode = true;
            }
            else if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            {
                IsSupported = true;
            }
        }

        public static int RecvFrom(IntPtr socketHandle, byte[] pinnedBuffer, int len, byte[] socketAddress, ref int socketAddressSize)
        {
            return UnixMode ? UnixSock.recvfrom(socketHandle, pinnedBuffer, len, 0, socketAddress, ref socketAddressSize) : WinSock.recvfrom(socketHandle, pinnedBuffer, len, 0, socketAddress, ref socketAddressSize);
        }

        public static unsafe int SendTo(IntPtr socketHandle, byte* pinnedBuffer, int len, byte[] socketAddress, int socketAddressSize)
        {
            return UnixMode ? UnixSock.sendto(socketHandle, pinnedBuffer, len, 0, socketAddress, socketAddressSize) : WinSock.sendto(socketHandle, pinnedBuffer, len, 0, socketAddress, socketAddressSize);
        }

        public static SocketError GetSocketError()
        {
            int error = Marshal.GetLastWin32Error();
            if (UnixMode)
                return NativeErrorToSocketError.TryGetValue(error, out SocketError err) ? err : SocketError.SocketError;
            return (SocketError)error;
        }

        public static SocketException GetSocketException()
        {
            int error = Marshal.GetLastWin32Error();
            if (UnixMode)
                return NativeErrorToSocketError.TryGetValue(error, out SocketError err) ? new((int)err) : new SocketException((int)SocketError.SocketError);
            return new(error);
        }

        public static short GetNativeAddressFamily(IPEndPoint remoteEndPoint)
        {
            return UnixMode ? (short)(remoteEndPoint.AddressFamily == AddressFamily.InterNetwork ? AF_INET : AF_INET6) : (short)remoteEndPoint.AddressFamily;
        }
    }
}