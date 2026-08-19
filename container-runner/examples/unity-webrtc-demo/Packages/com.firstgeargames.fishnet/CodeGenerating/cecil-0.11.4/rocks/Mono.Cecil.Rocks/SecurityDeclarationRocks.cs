//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

#if !NET_CORE
using System;
using System.Security;
using System.Security.Permissions;
using SSP = System.Security.Permissions;

namespace MonoFN.Cecil.Rocks
{
#if UNITY_EDITOR
    public
#endif
        static class SecurityDeclarationRocks
    {
        public static PermissionSet ToPermissionSet(this SecurityDeclaration self)
        {
            if (self == null)
                throw new ArgumentNullException("self");

            PermissionSet set;
            if (TryProcessPermissionSetAttribute(self, out set))
                return set;

            return CreatePermissionSet(self);
        }

        private static bool TryProcessPermissionSetAttribute(SecurityDeclaration declaration, out PermissionSet set)
        {
            set = null;

            if (!declaration.HasSecurityAttributes && declaration.SecurityAttributes.Count != 1)
                return false;

            SecurityAttribute security_attribute = declaration.SecurityAttributes[0];
            if (!security_attribute.AttributeType.IsTypeOf("System.Security.Permissions", "PermissionSetAttribute"))
                return false;

            PermissionSetAttribute attribute = new((SSP.SecurityAction)declaration.Action);

            CustomAttributeNamedArgument named_argument = security_attribute.Properties[0];
            string value = (string)named_argument.Argument.Value;
            switch (named_argument.Name)
            {
                case "XML":
                    attribute.XML = value;
                    break;
                case "Name":
                    attribute.Name = value;
                    break;
                default:
                    throw new NotImplementedException(named_argument.Name);
            }

            set = attribute.CreatePermissionSet();
            return true;
        }

        private static PermissionSet CreatePermissionSet(SecurityDeclaration declaration)
        {
            PermissionSet set = new(SSP.PermissionState.None);

            foreach (SecurityAttribute attribute in declaration.SecurityAttributes)
            {
                IPermission permission = CreatePermission(declaration, attribute);
                set.AddPermission(permission);
            }

            return set;
        }

        private static IPermission CreatePermission(SecurityDeclaration declaration, SecurityAttribute attribute)
        {
            Type attribute_type = Type.GetType(attribute.AttributeType.FullName);
            if (attribute_type == null)
                throw new ArgumentException("attribute");

            System.Security.Permissions.SecurityAttribute security_attribute = CreateSecurityAttribute(attribute_type, declaration);
            if (security_attribute == null)
                throw new InvalidOperationException();

            CompleteSecurityAttribute(security_attribute, attribute);

            return security_attribute.CreatePermission();
        }

        private static void CompleteSecurityAttribute(SSP.SecurityAttribute security_attribute, SecurityAttribute attribute)
        {
            if (attribute.HasFields)
                CompleteSecurityAttributeFields(security_attribute, attribute);

            if (attribute.HasProperties)
                CompleteSecurityAttributeProperties(security_attribute, attribute);
        }

        private static void CompleteSecurityAttributeFields(SSP.SecurityAttribute security_attribute, SecurityAttribute attribute)
        {
            Type type = security_attribute.GetType();

            foreach (CustomAttributeNamedArgument named_argument in attribute.Fields)
                type.GetField(named_argument.Name).SetValue(security_attribute, named_argument.Argument.Value);
        }

        private static void CompleteSecurityAttributeProperties(SSP.SecurityAttribute security_attribute, SecurityAttribute attribute)
        {
            Type type = security_attribute.GetType();

            foreach (CustomAttributeNamedArgument named_argument in attribute.Properties)
                type.GetProperty(named_argument.Name).SetValue(security_attribute, named_argument.Argument.Value, null);
        }

        private static SSP.SecurityAttribute CreateSecurityAttribute(Type attribute_type, SecurityDeclaration declaration)
        {
            SSP.SecurityAttribute security_attribute;
            try
            {
                security_attribute = (SSP.SecurityAttribute)Activator.CreateInstance(attribute_type, new object[] { (SSP.SecurityAction)declaration.Action });
            }
            catch (MissingMethodException)
            {
                security_attribute = (SSP.SecurityAttribute)Activator.CreateInstance(attribute_type, new object [0]);
            }

            return security_attribute;
        }

        public static SecurityDeclaration ToSecurityDeclaration(this PermissionSet self, SecurityAction action, ModuleDefinition module)
        {
            if (self == null)
                throw new ArgumentNullException("self");
            if (module == null)
                throw new ArgumentNullException("module");

            SecurityDeclaration declaration = new(action);

            SecurityAttribute attribute = new(module.TypeSystem.LookupType("System.Security.Permissions", "PermissionSetAttribute"));

            attribute.Properties.Add(new("XML", new(module.TypeSystem.String, self.ToXml().ToString())));

            declaration.SecurityAttributes.Add(attribute);

            return declaration;
        }
    }
}

#endif