namespace FishNet.Utility.Performance
{
    public static class RetrieveOptionExtensions
    {
        public static bool FastContains(this ObjectPoolRetrieveOption whole, ObjectPoolRetrieveOption part) => (whole & part) == part;
    }

    [System.Flags]
    public enum ObjectPoolRetrieveOption
    {
        Unset = 0,
        /// <summary>
        /// True to make the object active before returning.
        /// </summary>
        MakeActive = 1,
        /// <summary>
        /// True to treat supplied transform properties as local space.
        /// False will treat the properties as world space.
        /// </summary>
        LocalSpace = 2
    }
}