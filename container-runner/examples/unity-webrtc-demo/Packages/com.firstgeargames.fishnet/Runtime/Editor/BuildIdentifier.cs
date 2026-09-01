#if UNITY_EDITOR
using System.IO;
using UnityEditor;
using UnityEditor.Callbacks;

namespace FishNet.Editing
{
    /* When creating builds this will place an empty file within
     * the build folder.
     *
     * The file contains absolutely no information, and is used by our partners to identify how many of their customers are using
     * Fish-Networking.
     *
     * While this file is not required, you may delete the file and/or this code, we request that you please
     * consider keeping the file present as it helps keep FishNet free. */

    public class BuildIdentifier
    {
        [PostProcessBuild(1)]
        public static void OnPostprocessBuild(BuildTarget target, string pathToBuiltProject)
        {
            /* Previously only server builds were included, but it makes sense to include
             * in all builds for when used with client-auth relays. */
            string buildPath = Path.GetDirectoryName(pathToBuiltProject);
            if (buildPath == null)
                return;

            // Try to create the empty file.
            try
            {
                string filePath = Path.Combine(buildPath, "FishNet.SDK.Id");
                File.WriteAllText(filePath, string.Empty);
            }
            finally { }
        }
    }
}

#endif