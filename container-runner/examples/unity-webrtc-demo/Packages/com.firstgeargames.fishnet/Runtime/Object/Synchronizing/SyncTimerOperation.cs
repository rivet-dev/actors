namespace FishNet.Object.Synchronizing
{
    public enum SyncTimerOperation : byte
    {
        /// <summary>
        /// Timer is started. Value is included with start.
        /// </summary>
        Start = 1,
        /// <summary>
        /// Timer was paused.
        /// </summary>
        Pause = 2,
        /// <summary>
        /// Timer was paused. Value at time of pause is sent.
        /// </summary>
        PauseUpdated = 3,
        /// <summary>
        /// Timer was unpaused.
        /// </summary>
        Unpause = 4,
        /// <summary>
        /// Timer was stopped.
        /// </summary>
        Stop = 6,
        /// <summary>
        /// Timer was stopped. Value prior to stopping is sent.
        /// </summary>
        StopUpdated = 7,
        /// <summary>
        /// The timer has ended finished it's duration.
        /// </summary>
        Finished = 8,
        /// <summary>
        /// All operations for the tick have been processed. This only occurs on clients as the server is unable to be aware of when the user is done modifying the list.
        /// </summary>
        Complete = 9
    }
}