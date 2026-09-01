using CodeBoost.Extensions;

namespace CodeBoost.Environment
{

    public static class ApplicationStateService
    {
        /// <summary>
        /// Called when ApplicationState is set.
        /// </summary>
        internal static event ApplicationStateSetEventHandler ApplicationStateSet;

        internal delegate void ApplicationStateSetEventHandler(IApplicationState previousApplicationState, IApplicationState nextApplicationState);

        /// <summary>
        /// ILogger to use.
        /// </summary>
        internal static IApplicationState ApplicationState;

        /// <summary>
        /// Message when trying to access ApplicationState when there is not an instance created.
        /// </summary>
        private static readonly string ApplicationStateIsNullMessage = $"[{nameof(ApplicationState)}] is null. Use [{nameof(ApplicationStateService)}] to set a service.";
            
        static ApplicationStateService()
        {
            UseApplicationState(new IdeApplicationState());
        }

        /// <summary>
        /// Specifies which ILogger to use.
        /// </summary>
        public static void UseApplicationState(IApplicationState applicationState)
        {
            IApplicationState previousApplicationState = ApplicationState;
            ApplicationState = applicationState;
                
            ApplicationStateSet?.Invoke(previousApplicationState, applicationState);
        }

        /// <summary>
        /// True if the application is quitting.
        /// </summary>
        internal static bool IsQuitting()
        {
            if (ApplicationState is not null)
                return ApplicationState.IsQuitting();

            throw new(ApplicationStateIsNullMessage);
        }

        /// <summary>
        /// True if the application is playing.
        /// </summary>
        internal static bool IsPlaying()
        {
            if (ApplicationState is not null)
                return ApplicationState.IsQuitting();

            throw new(ApplicationStateIsNullMessage);
        }

        /// <summary>
        /// Quits the application for editor or builds.
        /// </summary>
        internal static void Quit()
        {
            if (ApplicationState is not null)
                ApplicationState.Quit();

            throw new(ApplicationStateIsNullMessage);
        }

        /// <summary>
        /// True if the application is being run within an editor.
        /// </summary>
        public static bool IsEditor()
        {
            if (ApplicationState is not null)
                return ApplicationState.IsEditor();

            throw new(ApplicationStateIsNullMessage);
        }

        /// <summary>
        /// True if the application is a build with development or debugging enabled.
        /// </summary>
        /// >
        public static bool IsDevelopmentBuild()
        {
            if (ApplicationState is not null)
                return ApplicationState.IsDevelopment();

            throw new(ApplicationStateIsNullMessage);
        }

        /// <summary>
        /// True if a GUI build, such as a client build.
        /// </summary>
        public static bool IsGuiBuild()
        {
            if (ApplicationState is not null)
                return ApplicationState.IsGuiBuild();

            throw new(ApplicationStateIsNullMessage);
        }

        /// <summary>
        /// True if a headless build, such as a server build.
        /// </summary>
        public static bool IsHeadlessBuild()
        {
            if (ApplicationState is not null)
                return ApplicationState.IsHeadlessBuild();

            throw new(ApplicationStateIsNullMessage);
        }

    }
}
