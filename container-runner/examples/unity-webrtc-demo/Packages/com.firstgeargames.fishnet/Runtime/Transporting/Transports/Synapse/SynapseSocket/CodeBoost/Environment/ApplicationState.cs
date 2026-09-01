using CodeBoost.Extensions;

namespace CodeBoost.Environment
{

    /// <summary>
    /// A static ApplicationState which uses the currently registered IApplicationState.
    /// </summary>
    public static class ApplicationState
    {
        static ApplicationState()
        {
            ApplicationStateService.ApplicationStateSet += ApplicationStateService_OnApplicationStateSet;

            /* If an IApplicationState is already set then call
             * value set callback to initialize for it. */
            if (ApplicationStateService.ApplicationState is not null)
                ApplicationStateService_OnApplicationStateSet(previousApplicationState: null, ApplicationStateService.ApplicationState);
        }

        private static void ApplicationStateService_OnApplicationStateSet(IApplicationState previousApplicationState, IApplicationState applicationState)
        {
            if (previousApplicationState is not null)
                previousApplicationState.FocusChanged -= ApplicationState_OnFocusChanged;

            if (applicationState is not null)
                applicationState.FocusChanged += ApplicationState_OnFocusChanged;
        }
            

        /// <summary>
        /// Called when the application focus state changes.
        /// </summary>
        public static event IApplicationState.FocusChangeEventHandler FocusChanged;

        /// <summary>
        /// Callback for when the 
        /// </summary>
        /// <param name="isFocused"></param>
        private static void ApplicationState_OnFocusChanged(bool isFocused) => FocusChanged?.Invoke(isFocused);
            
        /// <summary>
        /// True if the application is quitting.
        /// </summary>
        public static bool IsQuitting() => ApplicationStateService.IsQuitting();

        /// <summary>
        /// True if the application is playing.
        /// </summary>
        public static bool IsPlaying() =>  ApplicationStateService.IsPlaying();

        /// <summary>
        /// Quits the application for editor or builds.
        /// </summary>
        public static void Quit() => ApplicationStateService.Quit();

        /// <summary>
        /// True if the application is being run within an editor.
        /// </summary>
        public static bool IsEditor() => ApplicationStateService.IsEditor();

        /// <summary>
        /// True if the application is a build with development or debugging enabled.
        /// </summary>
        /// >
        public static bool IsDevelopmentBuild() => ApplicationStateService.IsDevelopmentBuild();
            
        /// <summary>
        /// True if a GUI build, such as a client build.
        /// </summary>
        public static bool IsGuiBuild() => ApplicationStateService.IsGuiBuild();
            
        /// <summary>
        /// True if a headless build, such as a server build.
        /// </summary>
        public static bool IsHeadlessBuild() => ApplicationStateService.IsHeadlessBuild();
    }
}
