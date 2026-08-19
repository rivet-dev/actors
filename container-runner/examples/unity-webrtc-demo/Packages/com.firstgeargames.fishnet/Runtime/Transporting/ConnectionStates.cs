namespace FishNet.Transporting
{
    /// <summary>
    /// States the local connection can be in.
    /// </summary>
    [System.Flags]
    public enum LocalConnectionState : int
    {
        /// <summary>
        /// Connection is fully stopped.
        /// </summary>
        Stopped = 1 << 0,
        /// <summary>
        /// Connection is stopping.
        /// </summary>
        Stopping = 1 << 1,
        /// <summary>
        /// Connection is starting but not yet established.
        /// </summary>
        Starting = 1 << 2,
        /// <summary>
        /// Connection is established.
        /// </summary>
        Started = 1 << 3

        // StoppedError = (1 << 4),
        // StoppedClosed = (1 << 5),
    }

    public static class LocalConnectionStateExtensions
    {
        /// <summary>
        /// True if the connection state is stopped or stopping.
        /// </summary>
        public static bool IsStoppedOrStopping(this LocalConnectionState connectionState) => connectionState == LocalConnectionState.Stopped || connectionState == LocalConnectionState.Stopping;

        /// <summary>
        /// True if the connection state is started or starting.
        /// </summary>
        public static bool IsStartedOrStarting(this LocalConnectionState connectionState) => connectionState == LocalConnectionState.Started || connectionState == LocalConnectionState.Starting;
    }

    /// <summary>
    /// States a remote client can be in.
    /// </summary>
    public enum RemoteConnectionState : byte
    {
        /// <summary>
        /// Connection is fully stopped.
        /// </summary>
        Stopped = 0,
        /// <summary>
        /// Connection is established.
        /// </summary>
        Started = 2
    }
}