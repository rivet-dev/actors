using System;
#if UNITY_WEBGL
using System.Runtime.InteropServices;
#endif

namespace cakeslice.SimpleWebRTC
{
	internal static class SimpleWebRTCJSLib
	{
#if UNITY_WEBGL
		[DllImport("__Internal")]
		internal static extern bool IsConnectedRTC(int index);

#pragma warning disable CA2101 // Specify marshaling for P/Invoke string arguments
		[DllImport("__Internal")]
#pragma warning restore CA2101 // Specify marshaling for P/Invoke string arguments
		internal static extern int ConnectRTC(string address, string iceServers, Action<int> openCallback, Action<int> closeCallBack, Action<int, IntPtr, int> messageCallback, Action<int> errorCallback);

		[DllImport("__Internal")]
		internal static extern void DisconnectRTC(int index);

		[DllImport("__Internal")]
		internal static extern bool SendRTC(int index, byte[] array, int offset, int length, int deliveryMethod);
#else
		internal static bool IsConnectedRTC(int index) => throw new NotSupportedException();

		internal static int ConnectRTC(string address, string iceServers,  Action<int> openCallback, Action<int> closeCallBack, Action<int, IntPtr, int> messageCallback, Action<int> errorCallback) => throw new NotSupportedException();

		internal static void DisconnectRTC(int index) => throw new NotSupportedException();

		internal static bool SendRTC(int index, byte[] array, int offset, int length, int deliveryMethod) => throw new NotSupportedException();
#endif
	}
}
