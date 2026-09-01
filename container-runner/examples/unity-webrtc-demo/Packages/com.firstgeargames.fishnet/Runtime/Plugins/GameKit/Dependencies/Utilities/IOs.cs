using System.Collections.Generic;
using System.IO;

namespace GameKit.Dependencies.Utilities
{
    public static class IOs
    {
        /// <summary>
        /// Finds all prefab files in a path.
        /// </summary>
        /// <param name = "startingPath">Path to begin searching in; this is typically "Assets".</param>
        /// <param name = "excludedPaths">Paths to exclude when searching.</param>
        /// <param name = "recursive">True to search subpaths.</param>
        /// <returns></returns>
        public static string[] GetDirectoryFiles(string startingPath, HashSet<string> excludedPaths, bool recursive, string extension)
        {
            // Opportunity to exit early if there are no excluded paths.
            if (excludedPaths.Count == 0)
            {
                string[] strResults = Directory.GetFiles(startingPath, extension, SearchOption.AllDirectories);
                return strResults;
            }
            // starting path is excluded.
            if (excludedPaths.Contains(startingPath))
                return new string[0];

            // Folders remaining to be iterated.
            List<string> enumeratedCollection = new() { startingPath };
            // Only check other directories if recursive.
            if (recursive)
            {
                // Find all folders which aren't excluded.
                for (int i = 0; i < enumeratedCollection.Count; i++)
                {
                    string[] allFolders = Directory.GetDirectories(enumeratedCollection[i], "*", SearchOption.TopDirectoryOnly);
                    for (int z = 0; z < allFolders.Length; z++)
                    {
                        string current = allFolders[z];
                        // Not excluded.
                        if (!excludedPaths.Contains(current))
                            enumeratedCollection.Add(current);
                    }
                }
            }

            // Valid prefab files.
            List<string> results = new();
            // Build files from folders.
            int count = enumeratedCollection.Count;
            for (int i = 0; i < count; i++)
            {
                string[] r = Directory.GetFiles(enumeratedCollection[i], extension, SearchOption.TopDirectoryOnly);
                results.AddRange(r);
            }

            return results.ToArray();
        }
    }
}