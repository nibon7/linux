// SPDX-License-Identifier: GPL-2.0

#include <linux/acpi.h>

void *rust_helper_acpi_get_drvdata(struct acpi_device *adev)
{
	return dev_get_drvdata(&adev->dev);
}

void rust_helper_acpi_set_drvdata(struct acpi_device *adev, void *data)
{
	dev_set_drvdata(&adev->dev, data);
}

void rust_helper_acpi_device_put(struct acpi_device *adev)
{
	if (!IS_ERR_OR_NULL(adev))
		put_device(&adev->dev);
}
