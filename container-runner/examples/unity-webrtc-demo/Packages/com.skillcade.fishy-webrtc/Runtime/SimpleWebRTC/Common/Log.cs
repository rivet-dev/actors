using System;
using UnityEngine;

namespace cakeslice.SimpleWebRTC
{
	public static class Log
	{
		public enum Levels
		{
			none = 0,
			error = 1,
			warn = 2,
			info = 3,
			verbose = 4,
		}

		public static Levels level = Levels.warn;

		public static string BufferToString(byte[] buffer, int offset = 0, int? length = null)
		{
			return BitConverter.ToString(buffer, offset, length ?? buffer.Length);
		}

		[System.Diagnostics.Conditional("DEBUG")]
		public static void DumpBuffer(string label, byte[] buffer, int offset, int length)
		{
			if (level < Levels.verbose)
				return;

			Debug.Log($"SimpleWebRTC Verbose: {label}: {BufferToString(buffer, offset, length)}");
		}

		[System.Diagnostics.Conditional("DEBUG")]
		public static void DumpBuffer(string label, ArrayBuffer arrayBuffer)
		{
			if (level < Levels.verbose)
				return;

			Debug.Log($"SimpleWebRTC Verbose: {label}: {BufferToString(arrayBuffer.array, 0, arrayBuffer.count)}");
		}

		public static void Verbose(string msg)
		{
			if (level < Levels.verbose)
				return;

			Debug.Log($"SimpleWebRTC Verbose: {msg}");
		}

		public static void Info(string msg)
		{
			if (level < Levels.info)
				return;

			Debug.Log($"SimpleWebRTC: {msg}");
		}

		public static void InfoException(Exception e)
		{
			if (level < Levels.info)
				return;

			Debug.Log($"SimpleWebRTC Exception: {e.GetType().Name} Message: {e.Message}\n{e.StackTrace}\n\n");
		}

		public static void Warn(string msg)
		{
			if (level < Levels.warn)
				return;

			Debug.LogWarning($"SimpleWebRTC: {msg}");
		}

		public static void Error(string msg)
		{
			if (level < Levels.error)
				return;

			Debug.LogError($"SimpleWebRTC: {msg}");
		}

		public static void Exception(Exception e)
		{
			if (level < Levels.error)
				return;

			Debug.LogError($"SimpleWebRTC Exception: {e.GetType().Name} Message: {e.Message}\n{e.StackTrace}\n\n");
		}
	}
}
