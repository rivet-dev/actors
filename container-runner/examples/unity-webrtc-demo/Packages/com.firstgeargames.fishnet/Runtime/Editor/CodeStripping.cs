#if UNITY_EDITOR
using FishNet.Configuring;
using System.IO;
using UnityEngine;
using System.Xml.Serialization;
using FishNet.Editing.PrefabCollectionGenerator;
using UnityEditor.Compilation;
using UnityEditor.Build.Reporting;
using UnityEditor;
using UnityEditor.Build;

namespace FishNet.Configuring
{
    public class CodeStripping
    {
        /// <summary>
        /// True if making a release build for client.
        /// </summary>
        public static bool ReleasingForClient => Configuration.Configurations.CodeStripping.IsBuilding && !Configuration.Configurations.CodeStripping.IsHeadless && !Configuration.Configurations.CodeStripping.IsDevelopment;
        /// <summary>
        /// True if making a release build for server.
        /// </summary>
        public static bool ReleasingForServer => Configuration.Configurations.CodeStripping.IsBuilding && Configuration.Configurations.CodeStripping.IsHeadless && !Configuration.Configurations.CodeStripping.IsDevelopment;
        /// <summary>
        /// Returns if to remove server logic.
        /// </summary>
        /// <returns></returns>
        public static bool RemoveServerLogic
        {
            get
            {

                /* This is to protect non pro users from enabling this
                 * without the extra logic code.  */
#pragma warning disable CS0162 // Unreachable code detected
                return false;
#pragma warning restore CS0162 // Unreachable code detected
            }
        }
        /// <summary>
        /// True if building and stripping is enabled.
        /// </summary>
        public static bool StripBuild
        {
            get
            {

                /* This is to protect non pro users from enabling this
                 * without the extra logic code.  */
#pragma warning disable CS0162 // Unreachable code detected
                return false;
#pragma warning restore CS0162 // Unreachable code detected
            }
        }
        /// <summary>
        /// Technique to strip methods.
        /// </summary>
        public static StrippingTypes StrippingType => (StrippingTypes)Configuration.Configurations.CodeStripping.StrippingType;
        private static object _compilationContext;
        public int callbackOrder => 0;

        public void OnPreprocessBuild(BuildReport report)
        {
            Generator.IgnorePostProcess = true;
            Generator.GenerateFull();
            CompilationPipeline.compilationStarted += CompilationPipelineOnCompilationStarted;
            CompilationPipeline.compilationFinished += CompilationPipelineOnCompilationFinished;

        }

        /* Solution for builds ending with errors and not triggering OnPostprocessBuild.
         * Link: https://gamedev.stackexchange.com/questions/181611/custom-build-failure-callback
         */
        private void CompilationPipelineOnCompilationStarted(object compilationContext)
        {
            _compilationContext = compilationContext;
        }

        private void CompilationPipelineOnCompilationFinished(object compilationContext)
        {
            if (compilationContext != _compilationContext)
                return;

            _compilationContext = null;

            CompilationPipeline.compilationStarted -= CompilationPipelineOnCompilationStarted;
            CompilationPipeline.compilationFinished -= CompilationPipelineOnCompilationFinished;

            // BuildingEnded();
        }

        private void BuildingEnded()
        {

            Generator.IgnorePostProcess = false;
        }

        public void OnPostprocessBuild(BuildReport report)
        {
                BuildingEnded();
        }
    }
}
#endif