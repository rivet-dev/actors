namespace CodeBoost.Environment
{

    public interface IApplicationState
    {
        /// <summary>
        /// Called when the application focus state changes.
        /// </summary>
        public event FocusChangeEventHandler FocusChanged;

        public delegate void FocusChangeEventHandler(bool isFocused);
            
        /// <summary>
        /// True if the application is quitting.
        /// </summary>
        public bool IsQuitting();

        /// <summary>
        /// True if the application is playing.
        /// </summary>
        public bool IsPlaying();

        /// <summary>
        /// Quits the application for editor or builds.
        /// </summary>
        public void Quit();

        /// <summary>
        /// True if the application is being run within an editor, false if a build.
        /// </summary>
        public bool IsEditor();

        /// <summary>
        /// True if the application is in development mode, false if release mode.
        /// </summary>
        /// <remarks>This should be true if running in the editor.</remarks>
        public bool IsDevelopment();

        /// <summary>
        /// True if a GUI build, such as a client build.
        /// </summary>
        public bool IsGuiBuild();

        /// <summary>
        /// True if a headless build, such as a server build.
        /// </summary>
        public bool IsHeadlessBuild();
    }
}
