using System;
using System.Runtime.InteropServices;
using CodeBoost.Logging;

namespace CodeBoost.Performance
{

    /// <summary>
    /// Returns the memory size of a managed type.
    /// </summary>
    public static class ManagedSizeOf<T0> where T0 : struct
    {
        /// <summary>
        /// Cached size value.
        /// </summary>
        // ReSharper disable once StaticMemberInGenericType
        public static int Value { get; private set; }

        static ManagedSizeOf()
        {
            Type type = typeof(T0);

            //Enums must be handled as the underlying numeric type.
            if (type.IsEnum)
                type = Enum.GetUnderlyingType(type);

            try
            {
                Value = Marshal.SizeOf(type);
            }
            catch (Exception ex)
            {
                /* An exception can occur if the type has a property
                 * which can take up memory dynamically, such as
                 * a String. */
                Value = 0;

                Logger.LogError(typeof(ManagedSizeOf<T0>), $"Type [{typeof(T0).FullName}] caused an exception: [{ex.Message}].");
            }
        }

    }
}
