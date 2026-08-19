namespace FishNet.Object.Synchronizing
{
    public enum SyncStopwatchOperation : byte
    {
        /// <summary>
        /// Stopwatch is started. Value is included with start.
        /// </summary>
        Start = 1,
        /// <summary>
        /// Stopwatch was paused.
        /// </summary>
        Pause = 2,
        /// <summary>
        /// Stopwatch was paused. Value at time of pause is sent.
        /// </summary>
        PauseUpdated = 3,
        /// <summary>
        /// Stopwatch was unpaused.
        /// </summary>
        Unpause = 4,
        /// <summary>
        /// Stopwatch was stopped.
        /// </summary>
        Stop = 6,
        /// <summary>
        /// Stopwatch was stopped. Value prior to stopping is sent.
        /// </summary>
        StopUpdated = 7,
        /// <summary>
        /// All operations for the tick have been processed. This only occurs on clients as the server is unable to be aware of when the user is done modifying the list.
        /// </summary>
        Complete = 9
    }
}