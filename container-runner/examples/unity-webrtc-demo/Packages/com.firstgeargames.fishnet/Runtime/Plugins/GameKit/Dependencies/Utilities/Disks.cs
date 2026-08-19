using System;
using System.IO;
using UnityEngine;

namespace GameKit.Dependencies.Utilities
{
    public static class Disks
    {
        /// <summary>
        /// Writes specified text to a file path.
        /// </summary>
        /// <param name = "text"></param>
        /// <param name = "path"></param>
        /// <param name = "formatPath">True to format the path to the current platform.</param>
        public static void WriteToFile(string text, string path, bool formatPath = true)
        {
            // If to format the path for the platform.
            if (formatPath)
                path = FormatPlatformPath(path);

            // Path came back or was passed in as an empty string.
            if (path == string.Empty)
            {
                Debug.LogError("Path cannot be null.");
                return;
            }

            try
            {
                // Get directory path.
                string directory = Path.GetDirectoryName(path);
                // If directory doesn't exist try to create it.
                if (!Directory.Exists(directory))
                    Directory.CreateDirectory(directory);

                //Try to write the file data.
                using (FileStream fs = new(path, FileMode.Create))
                {
                    using (StreamWriter writer = new(fs))
                    {
                        writer.Write(text);
                    }
                }
            }
            catch (Exception ex)
            {
                Debug.LogError($"An error occured during a file write. Error: {ex.Message} {Environment.NewLine} File path: {path} {Environment.NewLine} Text: {text}");
            }

            /* If within the editor then refresh the asset database so changes
             * reflect in the project folder. */
#if UNITY_EDITOR
            UnityEditor.AssetDatabase.Refresh();
#endif
        }

        /// <summary>
        /// Formats a file path to the current platform.
        /// </summary>
        /// <param name = "path"></param>
        /// <returns></returns>
        public static string FormatPlatformPath(string path)
        {
            //No path specified.
            if (path == string.Empty)
            {
                Debug.LogError("Path cannot be empty.");
                return string.Empty;
            }

            string convertedPath = string.Empty;

            //Get the directories as an array.
            string[] directories = path.Split(Path.DirectorySeparatorChar);

            //Go through each directory.
            for (int i = 0; i < directories.Length; i++)
            {
                /* If only one entry in array then the path
                 * is in the root of the Resources folder. */
                if (directories.Length == 1)
                {
                    //Append to converted path and break from the loop.
                    convertedPath = directories[i];
                    break;
                }
                //More than one entry, meaning there are sub paths.
                else
                {
                    /* Set converted path to the current
                     * convertedPath combined with the next directory. */
                    convertedPath = Path.Combine(convertedPath, directories[i]);
                }
            }

            return convertedPath;
        }
    }
}