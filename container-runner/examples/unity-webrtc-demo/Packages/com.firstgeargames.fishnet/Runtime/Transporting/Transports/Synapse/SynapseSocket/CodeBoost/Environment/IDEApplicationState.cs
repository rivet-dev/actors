namespace CodeBoost.Environment
{

    /// <summary>
    /// Provides application states for development within an IDE.
    /// </summary>
    public class IdeApplicationState : IApplicationState
    {
        /// <summary>
        /// Called when the application focus state changes.
        /// </summary>
        /// <remarks>Event never invokes for this type.</remarks>
        public event IApplicationState.FocusChangeEventHandler FocusChanged;

        /// <summary>
        /// Returns the value of System.Environment.HasShutdownStarted. 
        /// </summary>
        /// <returns>System.Environment.HasShutdownStarted.</returns>
        public bool IsQuitting() => System.Environment.HasShutdownStarted;

        /// <summary>
        /// Returns if IsQuitting is false.
        /// </summary>
        /// <returns>True if IsQuitting is false.</returns>
        public bool IsPlaying() => !IsQuitting();

        /// <summary>
        /// Exits System.Environment.
        /// </summary>
        public void Quit()
        {
            System.Environment.Exit(exitCode: 0);
        }

        /// <summary>
        /// Unconditionally returns true.
        /// </summary>
        /// <returns>True.</returns>
        public bool IsEditor() => true;

        /// <summary>
        /// Returns if the DEBUG preprocessor is active.
        /// </summary>
        /// <returns>Active state of Preprocessor DEBUG.</returns>
        public bool IsDevelopment()
        {
            #if DEBUG
            return true;
            #else
                return false;
            #endif
        }

        /// <summary>
        /// Unconditionally returns false.
        /// </summary>
        /// <returns>False.</returns>
        public bool IsGuiBuild() => false;

        /// <summary>
        /// Unconditionally returns false.
        /// </summary>
        /// <returns>False.</returns>
        public bool IsHeadlessBuild() => false;
    }
}
