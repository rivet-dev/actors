namespace GameKit.Dependencies.Utilities
{
    public static class Booleans
    {
        /// <summary>
        /// Converts a boolean to an integer, 1 for true 0 for false.
        /// </summary>
        public static int ToInt(this bool b)
        {
            return b ? 1 : 0;
        }
    }
}